//! Item pages (R-UI-6, R-UI-7): movie, series, season, episode. Placeholder look; every value from `Theme`.
use super::admin_actions;
use super::at_details::{self as at, ActionBtn, HeroArt, RoundIcon};
use super::collection;
use super::parts::{self, CardKind};
use super::player::{self, PlayOptions};
use super::playlists;
use super::show_page;
use super::widgets;
use crate::api::{
    image_width_bucket, secs_to_ticks, ticks_to_secs, ApiError, ImageKind, Item, ItemKind, MediaStream, MediaUrl,
    PersonRef, Result, SourceInfo, StreamKind,
};
use crate::app::{Cx, Screen};
use crate::nav::{move_focus, Focus, Group};
use crate::platform::Key;

const G_MORE: usize = 0;
const G_BTN: usize = 1;
const G_LIST: usize = 2;
const G_CHIPS: usize = 3;
const G_CHAPTERS: usize = 4;
const G_PARTS: usize = 5;
const G_PEOPLE: usize = 6;
const G_SPECIALS: usize = 7;
const G_SIMILAR: usize = 8;
const G_INFO: usize = 9;
const G_LINKS: usize = 10;
const G_RETRY: usize = 11;
const G_POS: usize = 12;
const G_STORY: usize = 13;
/// P-ITEM-2: an overview longer than this many lines is cut with a More / Less button.
const OVERVIEW_LINES: usize = 6;

const POS_MIN: i64 = 0;
const POS_MAX: i64 = 1_000_000_000;
const POS_STEP: i64 = 10;
const STORY_MAX: i64 = 100_000;

#[derive(Clone, Copy)]
enum Action {
    Play { from_start: bool },
    PlayNext,
    ToggleWatched,
    OpenAudio,
    OpenSubtitles,
    AudioPick(i32),
    SubtitlePick(Option<i32>),
    OpenSection,
    SectionPick(usize),
    // F4 (R-F4-2): the "Position…" number box on the item page (admin, opened from a library).
    OpenPosition,
    PositionSave,
    PositionCancel,
    // F3 (R-F3-3): the "Story year…" editor on the item page (admin).
    OpenStory,
    StorySave,
    StoryClear,
    StoryCancel,
    StoryEra,
    // G4 (P-TV-9): previous / next episode on an episode page.
    PlayPrevEp,
    PlayNextEp,
    // G4 (P-ITEM-17): play from a chapter (start position in seconds).
    PlayChapter(f64),
    ToggleFavorite,
    // A6 (P-ITEM-20): the "More…" screen (edit, identify, refresh, merge / split, copy link, delete).
    OpenMore,
    // G8 (P-ITEM-8, P-ITEM-11)
    Trailer,
    OpenVersion,
    VersionPick(usize),
    // G7 (P-COLL-2/3/5): the "Add to…" menu.
    OpenAddTo,
    AddPick(AddAct),
}

/// G8 / P-ITEM-8: where the Trailer button goes.
#[derive(Debug, Clone, PartialEq)]
pub enum TrailerTarget {
    /// A trailer in the library: played in the player.
    Local(Box<Item>),
    /// A web URL (YouTube …): opened in the browser.
    Remote(String),
}

/// G7: which level of the "Add to…" menu is showing.
#[derive(Clone, Copy, PartialEq)]
enum AddMode {
    Root,
    Playlists,
    Collections,
}

/// G7: what an "Add to…" menu row does.
#[derive(Clone, Copy)]
enum AddAct {
    Mode(AddMode),
    NewPlaylist,
    NewCollection,
    ToPlaylist(usize),
    ToCollection(usize),
    FromCollection(usize),
}

pub struct ItemScreen {
    item: Item,
    kind: ItemKind,
    full: Option<Item>,
    loading: bool,
    error: Option<String>,
    seasons: Vec<Item>,
    episodes: Vec<Item>,
    next_up: Vec<Item>,
    audio: Option<i32>,
    subtitle: Option<Option<i32>>,
    tracks_init: bool,
    audio_open: bool,
    subtitle_open: bool,
    pending_open: Option<String>,
    is_admin: bool,
    section: admin_actions::SectionMove,
    // F4 (R-F4-2): the place (library/collection id) this item page was opened from; "Position…" is hidden
    // when None.
    place: Option<String>,
    position: admin_actions::PositionMove,
    story: admin_actions::StoryEdit,
    // G4 (P-TV-9): the neighbouring episodes of an episode page.
    prev_ep: Option<Item>,
    next_ep: Option<Item>,
    // G8: trailers, extras, similar items, media info and the version choice.
    trailers: Vec<Item>,
    specials: Vec<Item>,
    /// RR6 (P-ITEM-16): the other parts of a multi-part film.
    parts: Vec<Item>,
    similar: Vec<Item>,
    media_info: Vec<SourceInfo>,
    version: usize,
    version_open: bool,
    // G7: the "Add to…" menu, the playlists / collections it offers and the collections this item is in.
    add_open: bool,
    add_mode: AddMode,
    playlists: Option<Vec<Item>>,
    collections: Option<Vec<Item>>,
    in_collections: Vec<Item>,
    add_note: Option<String>,
    // RR5 (P-ITEM-2): the overview is showing in full.
    overview_open: bool,
    focus: Focus,
    groups: Vec<Group>,
    started: bool,
}

pub fn screen(item: Item) -> Box<dyn Screen> {
    screen_in(item, None)
}

/// Like `screen`, but remembers the place (library/collection id) the item was opened from so the admin
/// "Position…" control can write the right `rf-order` tag (R-F4-2). Series and seasons ignore it.
pub fn screen_in(item: Item, place: Option<String>) -> Box<dyn Screen> {
    match &item.kind {
        ItemKind::Series => show_page::screen(item, None),
        ItemKind::Season => show_page::screen_for_season(item),
        // G7 (P-COLL-1): a collection opens its own page.
        ItemKind::BoxSet => collection::screen(item),
        _ => {
            let kind = item.kind.clone();
            Box::new(ItemScreen {
                item,
                kind,
                full: None,
                loading: true,
                error: None,
                seasons: vec![],
                episodes: vec![],
                next_up: vec![],
                audio: None,
                subtitle: None,
                tracks_init: false,
                audio_open: false,
                subtitle_open: false,
                pending_open: None,
                is_admin: false,
                section: admin_actions::SectionMove::new(),
                place,
                position: admin_actions::PositionMove::new(),
                story: admin_actions::StoryEdit::new(),
                prev_ep: None,
                next_ep: None,
                trailers: vec![],
                specials: vec![],
                parts: vec![],
                similar: vec![],
                media_info: vec![],
                version: 0,
                version_open: false,
                add_open: false,
                add_mode: AddMode::Root,
                playlists: None,
                collections: None,
                in_collections: vec![],
                add_note: None,
                overview_open: false,
                focus: Focus { group: G_BTN, index: 0 },
                groups: vec![],
                started: false,
            })
        }
    }
}

impl ItemScreen {
    fn current(&self) -> &Item {
        self.full.as_ref().unwrap_or(&self.item)
    }

    fn source(&self) -> Option<&crate::api::MediaSource> {
        let sources = &self.current().media_sources;
        sources.get(self.version).or_else(|| sources.first())
    }

    // G8: any of the dropdown lists open.
    fn picker_open(&self) -> bool {
        self.audio_open
            || self.subtitle_open
            || self.version_open
            || self.section.open
            || self.position.open
            || self.story.open
            || self.add_open
    }

    fn close_pickers(&mut self) {
        self.audio_open = false;
        self.subtitle_open = false;
        self.version_open = false;
        self.section.open = false;
        self.position.open = false;
        self.story.open = false;
        self.add_open = false;
    }

    // G8 / P-ITEM-8: the Trailer button's target, local first.
    fn trailer(&self) -> Option<TrailerTarget> {
        trailer_target(&self.trailers, &self.current().remote_trailers)
    }

    // G8 / P-ITEM-11: "Version N" until the media info names the sources.
    fn version_entries(&self) -> Vec<(String, usize)> {
        let n = self.current().media_sources.len();
        (0..n).map(|i| (version_label(i, self.current().media_sources.get(i), &self.media_info), i)).collect()
    }

    fn init_tracks(&mut self) {
        if self.tracks_init {
            return;
        }
        let (default_audio, default_subtitle, first_audio) = match self.source() {
            Some(ms) => (
                ms.default_audio_stream_index,
                ms.default_subtitle_stream_index,
                ms.media_streams.iter().find(|s| s.kind == StreamKind::Audio).map(|s| s.index),
            ),
            None => (None, None, None),
        };
        self.audio = Some(default_audio.or(first_audio).unwrap_or(0));
        self.subtitle = Some(default_subtitle.map(Some).unwrap_or(None));
        self.tracks_init = true;
    }

    fn stream_label_for(&self, s: &MediaStream) -> String {
        let mut l =
            s.display_title.clone().or_else(|| s.language.clone()).unwrap_or_else(|| format!("Track {}", s.index));
        if s.is_external && !l.to_lowercase().contains("external") {
            l.push_str(" · External");
        }
        l
    }

    fn stream_label(&self, kind: StreamKind, index: i32) -> Option<String> {
        let ms = self.source()?;
        ms.media_streams.iter().find(|s| s.kind == kind && s.index == index).map(|s| self.stream_label_for(s))
    }

    fn audio_label(&self) -> String {
        match self.audio {
            Some(i) => self.stream_label(StreamKind::Audio, i).unwrap_or_else(|| "Default".to_string()),
            None => "Default".to_string(),
        }
    }

    fn subtitle_label(&self) -> String {
        match self.subtitle {
            Some(Some(i)) => self.stream_label(StreamKind::Subtitle, i).unwrap_or_else(|| "Default".to_string()),
            Some(None) => "Off".to_string(),
            None => "Default".to_string(),
        }
    }

    fn audio_entries(&self) -> Vec<(String, i32)> {
        let mut v = vec![];
        if let Some(ms) = self.source() {
            for s in ms.media_streams.iter().filter(|s| s.kind == StreamKind::Audio) {
                v.push((self.stream_label_for(s), s.index));
            }
        }
        v
    }

    fn subtitle_entries(&self) -> Vec<(String, Option<i32>)> {
        let mut v = vec![("Off".to_string(), None)];
        if let Some(ms) = self.source() {
            for s in ms.media_streams.iter().filter(|s| s.kind == StreamKind::Subtitle) {
                v.push((self.stream_label_for(s), Some(s.index)));
            }
        }
        v
    }

    fn movie_buttons(&self) -> Vec<(String, Action)> {
        let mut v = vec![];
        let pos = self.current().user_data.as_ref().map(|u| u.playback_position_ticks).unwrap_or(0);
        if pos > 0 {
            v.push((format!("Resume from {}", parts::clock(ticks_to_secs(pos))), Action::Play { from_start: false }));
            v.push(("Play from start".to_string(), Action::Play { from_start: true }));
        } else {
            v.push(("Play".to_string(), Action::Play { from_start: true }));
        }
        // G8 (P-ITEM-8)
        if self.trailer().is_some() {
            v.push(("Trailer".to_string(), Action::Trailer));
        }
        // G4 (P-TV-9): previous / next episode in series order, next to Play on an episode page.
        if self.kind == ItemKind::Episode {
            if self.prev_ep.is_some() {
                v.push(("Previous episode".to_string(), Action::PlayPrevEp));
            }
            if self.next_ep.is_some() {
                v.push(("Next episode".to_string(), Action::PlayNextEp));
            }
        }
        let played = self.current().user_data.as_ref().map(|u| u.played).unwrap_or(false);
        v.push((if played { "Mark unwatched" } else { "Mark watched" }.to_string(), Action::ToggleWatched));
        // G8 (P-ITEM-11): only when the item has more than one version.
        if self.current().media_sources.len() > 1 {
            let label = self.version_entries().get(self.version).map(|(l, _)| l.clone()).unwrap_or_default();
            v.push((format!("Version: {label}"), Action::OpenVersion));
        }
        v.push((format!("Audio: {}", self.audio_label()), Action::OpenAudio));
        v.push((format!("Subtitles: {}", self.subtitle_label()), Action::OpenSubtitles));
        if self.is_admin {
            v.push((admin_actions::button_label(self.current()), Action::OpenSection));
            if let Some(place) = self.place.clone() {
                v.push((admin_actions::position_label(self.current(), &place), Action::OpenPosition));
            }
            v.push((admin_actions::story_label(self.current()), Action::OpenStory));
        }
        v.push(("Add to…".to_string(), Action::OpenAddTo));
        v.push(("".to_string(), Action::ToggleFavorite));
        v.push(("More…".to_string(), Action::OpenMore));
        v
    }

    /// D2: `movie_buttons` in the Apple TV look, same order (key paths stay): the first is the white Play / Resume
    /// pill, pickers are glass pills with a chevron, the rest round icon buttons with a caption under focus.
    fn action_btns(&self) -> Vec<(ActionBtn, Action)> {
        let played = self.current().user_data.as_ref().map(|u| u.played).unwrap_or(false);
        let fav = self.current().user_data.as_ref().map(|u| u.is_favorite).unwrap_or(false);
        let round = |icon, on, caption: &str| ActionBtn::Round { icon, on, caption: caption.to_string() };
        self.movie_buttons()
            .into_iter()
            .enumerate()
            .map(|(i, (label, a))| {
                let b = match a {
                    Action::Play { .. } if i == 0 => ActionBtn::Play(label.clone()),
                    Action::Play { .. } => round(RoundIcon::Restart, false, &label),
                    Action::Trailer => round(RoundIcon::Trailer, false, &label),
                    Action::PlayPrevEp => round(RoundIcon::Previous, false, &label),
                    Action::PlayNextEp => round(RoundIcon::Next, false, &label),
                    Action::ToggleWatched => round(RoundIcon::Watched, played, &label),
                    Action::OpenVersion | Action::OpenAudio | Action::OpenSubtitles => ActionBtn::Menu(label.clone()),
                    Action::OpenAddTo => round(RoundIcon::Add, false, &label),
                    Action::ToggleFavorite => round(RoundIcon::Favourite, fav, "Favourite"),
                    Action::OpenMore => round(RoundIcon::More, false, &label),
                    _ => ActionBtn::Pill(label.clone()),
                };
                (b, a)
            })
            .collect()
    }

    /// G7: the rows of the "Add to…" menu at its current level.
    fn add_entries(&self) -> Vec<(String, AddAct)> {
        match self.add_mode {
            AddMode::Root => {
                let mut v = vec![
                    ("Add to playlist…".to_string(), AddAct::Mode(AddMode::Playlists)),
                    ("New playlist…".to_string(), AddAct::NewPlaylist),
                ];
                if self.is_admin {
                    v.push(("Add to collection…".to_string(), AddAct::Mode(AddMode::Collections)));
                    v.push(("New collection…".to_string(), AddAct::NewCollection));
                    for (i, c) in self.in_collections.iter().enumerate() {
                        v.push((format!("Remove from {}", c.name), AddAct::FromCollection(i)));
                    }
                }
                v
            }
            AddMode::Playlists => self
                .playlists
                .iter()
                .flatten()
                .enumerate()
                .map(|(i, p)| (p.name.clone(), AddAct::ToPlaylist(i)))
                .collect(),
            AddMode::Collections => self
                .collections
                .iter()
                .flatten()
                .enumerate()
                .filter(|(_, c)| !self.in_collections.iter().any(|m| m.id == c.id))
                .map(|(i, c)| (c.name.clone(), AddAct::ToCollection(i)))
                .collect(),
        }
    }

    fn open_add_menu(&mut self, cx: &mut Cx) {
        self.add_open = true;
        self.add_mode = AddMode::Root;
        self.add_note = None;
        self.audio_open = false;
        self.subtitle_open = false;
        self.section.open = false;
        self.playlists = None;
        self.collections = None;
        let client = cx.api();
        cx.spawn("playlists", move || client.playlists());
        if self.is_admin {
            let client = cx.api();
            cx.spawn("collections", move || client.collections());
        }
    }

    fn pick_add(&mut self, cx: &mut Cx, act: AddAct) {
        let id = self.item.id.clone();
        match act {
            AddAct::Mode(m) => {
                self.add_mode = m;
                self.focus = Focus { group: G_LIST, index: 0 };
                return;
            }
            AddAct::NewPlaylist => {
                self.add_open = false;
                cx.push(playlists::new_screen(vec![id], self.item.name.clone()));
            }
            AddAct::NewCollection => {
                self.add_open = false;
                cx.push(playlists::new_collection_screen(vec![id], self.item.name.clone()));
            }
            AddAct::ToPlaylist(i) => {
                let Some(p) = self.playlists.as_ref().and_then(|v| v.get(i)).cloned() else { return };
                self.add_open = false;
                let client = cx.api();
                cx.spawn("add-done", move || {
                    client.add_to_playlist(&p.id, &[id]).map(|_| format!("Added to the playlist {}", p.name))
                });
            }
            AddAct::ToCollection(i) => {
                let Some(c) = self.collections.as_ref().and_then(|v| v.get(i)).cloned() else { return };
                self.add_open = false;
                let client = cx.api();
                cx.spawn("add-done", move || {
                    client.add_to_collection(&c.id, &[id]).map(|_| format!("Added to the collection {}", c.name))
                });
            }
            AddAct::FromCollection(i) => {
                let Some(c) = self.in_collections.get(i).cloned() else { return };
                self.add_open = false;
                let client = cx.api();
                cx.spawn("add-done", move || {
                    client
                        .remove_from_collection(&c.id, &[id])
                        .map(|_| format!("Removed from the collection {}", c.name))
                });
            }
        }
        self.focus = Focus { group: G_BTN, index: self.add_button_index() };
    }

    fn add_button_index(&self) -> usize {
        self.movie_buttons().iter().position(|(_, a)| matches!(a, Action::OpenAddTo)).unwrap_or(0)
    }

    fn load_in_collections(&self, cx: &Cx) {
        let id = self.item.id.clone();
        let client = cx.api();
        cx.spawn("in-collections", move || client.collections_containing(&id));
    }

    fn audio_button_index(&self) -> usize {
        self.movie_buttons().iter().position(|(_, a)| matches!(a, Action::OpenAudio)).unwrap_or(0)
    }

    fn subtitle_button_index(&self) -> usize {
        self.movie_buttons().iter().position(|(_, a)| matches!(a, Action::OpenSubtitles)).unwrap_or(0)
    }

    fn section_button_index(&self) -> usize {
        self.movie_buttons().iter().position(|(_, a)| matches!(a, Action::OpenSection)).unwrap_or(0)
    }

    fn position_button_index(&self) -> usize {
        self.movie_buttons().iter().position(|(_, a)| matches!(a, Action::OpenPosition)).unwrap_or(0)
    }

    fn story_button_index(&self) -> usize {
        self.movie_buttons().iter().position(|(_, a)| matches!(a, Action::OpenStory)).unwrap_or(0)
    }

    fn play_options(&self, from_start: bool) -> PlayOptions {
        PlayOptions {
            from_start,
            audio: self.audio.or(Some(0)),
            subtitle: self.subtitle.or(Some(None)),
            // G8 (P-ITEM-11): the chosen version, only when there is a choice.
            media_source_id: (self.current().media_sources.len() > 1)
                .then(|| self.source().map(|m| m.id.clone()))
                .flatten(),
        }
    }

    fn above_line(&self, item: &Item) -> Option<String> {
        match item.kind {
            ItemKind::Episode => {
                let series = item.series_name.clone().unwrap_or_else(|| item.name.clone());
                let s = item.parent_index_number?;
                let e = item.index_number?;
                Some(format!("{series} · S{s} · E{e}"))
            }
            ItemKind::Season => item.series_name.clone(),
            _ => None,
        }
    }

    fn refresh(&mut self, cx: &mut Cx) {
        let id = self.item.id.clone();
        let client = cx.api();
        cx.spawn("item", move || client.item(&id));
    }

    fn spawn_loads(&mut self, cx: &mut Cx) {
        self.refresh(cx);
        // G8: the media info panel + version labels on movie and episode pages; extras on movies.
        if matches!(self.kind, ItemKind::Movie | ItemKind::Episode) {
            let id = self.item.id.clone();
            let client = cx.api();
            cx.spawn("media-info", move || client.media_info(&id));
        }
        if self.kind == ItemKind::Movie {
            let id = self.item.id.clone();
            let client = cx.api();
            cx.spawn("trailers", move || client.local_trailers(&id));
            let id = self.item.id.clone();
            let client = cx.api();
            cx.spawn("specials", move || client.special_features(&id));
            let id = self.item.id.clone();
            let client = cx.api();
            cx.spawn("parts", move || client.additional_parts(&id));
            let id = self.item.id.clone();
            let client = cx.api();
            cx.spawn("similar", move || client.similar(&id, 12));
        }
        match self.kind {
            ItemKind::Series => {
                let sid = self.item.id.clone();
                let client = cx.api();
                let sid2 = sid.clone();
                cx.spawn("seasons", move || client.seasons(&sid2));
                let client = cx.api();
                let (rw, nd) = (cx.settings.next_up_rewatching, cx.settings.next_up_days);
                cx.spawn("nextup", move || client.next_up_with(20, rw, nd));
                let client = cx.api();
                cx.spawn("episodes", move || client.episodes(&sid, None));
            }
            ItemKind::Season => {
                let sid = self.item.series_id.clone().unwrap_or_default();
                let season = self.item.id.clone();
                let client = cx.api();
                cx.spawn("episodes", move || client.episodes(&sid, Some(&season)));
            }
            // G7 (P-COLL-3): the collections a film is in.
            ItemKind::Movie => self.load_in_collections(cx),
            // G4 (P-TV-9): the neighbours of an episode, for Previous / Next episode buttons.
            ItemKind::Episode => {
                if let Some(sid) = self.item.series_id.clone() {
                    let eid = self.item.id.clone();
                    let client = cx.api();
                    let sid2 = sid.clone();
                    cx.spawn("prev-ep", move || client.previous_episode(&sid2, &eid));
                    let eid = self.item.id.clone();
                    let client = cx.api();
                    cx.spawn("next-ep", move || client.next_episode(&sid, &eid));
                }
            }
            _ => {}
        }
    }

    fn toggle_watched(&mut self, cx: &mut Cx) {
        let played = self.current().user_data.as_ref().map(|u| u.played).unwrap_or(false);
        let id = self.item.id.clone();
        let client = cx.api();
        cx.spawn("played", move || client.set_played(&id, !played));
    }

    fn toggle_favorite(&mut self, cx: &mut Cx) {
        let fav = self.current().user_data.as_ref().map(|u| u.is_favorite).unwrap_or(false);
        let id = self.item.id.clone();
        let client = cx.api();
        cx.spawn("favorite", move || client.set_favorite(&id, !fav));
    }

    fn play_next_episode(&mut self, cx: &mut Cx) {
        let ep = self.next_up.iter().find(|e| e.series_id.as_deref() == Some(self.item.id.as_str())).cloned().or_else(
            || {
                let mut eps = self.episodes.clone();
                eps.sort_by_key(|e| (e.parent_index_number.unwrap_or(0), e.index_number.unwrap_or(0)));
                eps.into_iter().next()
            },
        );
        if let Some(ep) = ep {
            cx.push(player::screen(ep, PlayOptions::default()));
        }
    }

    /// RR6 (P-ITEM-16): a multi-part film plays as a queue (this part, then the others), starting at `start`
    /// (0 = the film's own first part); each part reports its own progress.
    fn play_parts(&mut self, cx: &mut Cx, start: usize, opts: PlayOptions) {
        let mut queue = vec![self.current().clone()];
        queue.extend(self.parts.iter().cloned());
        cx.push(player::screen_queue_at(queue, start, opts));
    }

    // G4 (P-ITEM-17): play from a chapter by reusing the resume position (avoids widening `PlayOptions`).
    fn play_chapter(&mut self, cx: &mut Cx, secs: f64) {
        let mut item = self.current().clone();
        let ud = item.user_data.get_or_insert_with(Default::default);
        ud.playback_position_ticks = secs_to_ticks(secs);
        let opts = self.play_options(false);
        cx.push(player::screen(item, opts));
    }

    /// Keys while the F4 "Position…" number box is open: digits and Up/Down edit the value, Enter saves,
    /// Back cancels.
    fn position_key(&mut self, cx: &mut Cx, key: Key) -> bool {
        let editing = self.focus.group == G_POS && self.focus.index == 0;
        match key {
            Key::Text(c) if c.is_ascii_digit() => {
                if editing {
                    self.position.value = widgets::number_key(self.position.value, key, POS_MIN, POS_MAX, POS_STEP);
                }
                true
            }
            Key::Up | Key::Down => {
                if editing {
                    self.position.value = widgets::number_key(self.position.value, key, POS_MIN, POS_MAX, POS_STEP);
                } else {
                    self.focus = move_focus(&self.groups, self.focus, key);
                }
                true
            }
            Key::Left | Key::Right => {
                self.focus = move_focus(&self.groups, self.focus, key);
                true
            }
            Key::Enter => {
                if self.focus.group == G_POS && self.focus.index == 2 {
                    self.do_action(cx, Action::PositionCancel);
                } else {
                    self.do_action(cx, Action::PositionSave);
                }
                true
            }
            Key::Back => {
                self.position.open = false;
                self.focus = Focus { group: G_BTN, index: self.position_button_index() };
                true
            }
            _ => false,
        }
    }

    /// Keys while the F3 "Story year…" editor is open: digits and Up/Down edit the number, Enter on the era
    /// button flips BBY/ABY, Enter elsewhere does what the control says, Back cancels.
    fn story_key(&mut self, cx: &mut Cx, key: Key) -> bool {
        let editing = self.focus.group == G_STORY && self.focus.index == 0;
        match key {
            Key::Text(c) if c.is_ascii_digit() => {
                if editing {
                    self.story.value = widgets::number_key(self.story.value, key, 0, STORY_MAX, 1);
                }
                true
            }
            Key::Up | Key::Down => {
                if editing {
                    self.story.value = widgets::number_key(self.story.value, key, 0, STORY_MAX, 1);
                } else {
                    self.focus = move_focus(&self.groups, self.focus, key);
                }
                true
            }
            Key::Left | Key::Right => {
                self.focus = move_focus(&self.groups, self.focus, key);
                true
            }
            Key::Enter => {
                let a = match self.focus.index {
                    1 => Action::StoryEra,
                    3 => Action::StoryClear,
                    4 => Action::StoryCancel,
                    _ => Action::StorySave,
                };
                self.do_action(cx, a);
                true
            }
            Key::Back => {
                self.do_action(cx, Action::StoryCancel);
                true
            }
            _ => false,
        }
    }

    fn do_action(&mut self, cx: &mut Cx, a: Action) {
        match a {
            Action::Play { from_start } => {
                let item = self.current().clone();
                let opts = self.play_options(from_start);
                if self.parts.is_empty() {
                    cx.push(player::screen(item, opts));
                } else {
                    self.play_parts(cx, 0, opts);
                }
            }
            Action::PlayNext => self.play_next_episode(cx),
            Action::ToggleWatched => self.toggle_watched(cx),
            Action::OpenAudio => {
                self.close_pickers();
                self.audio_open = true;
            }
            Action::OpenSubtitles => {
                self.close_pickers();
                self.subtitle_open = true;
            }
            // G8 (P-ITEM-11)
            Action::OpenVersion => {
                self.close_pickers();
                self.version_open = true;
            }
            Action::VersionPick(i) => {
                self.version = i;
                self.version_open = false;
                // the new version has its own tracks
                self.tracks_init = false;
            }
            // G8 (P-ITEM-8)
            Action::Trailer => match self.trailer() {
                Some(TrailerTarget::Local(it)) => {
                    cx.push(player::screen(*it, PlayOptions { from_start: true, ..Default::default() }));
                }
                Some(TrailerTarget::Remote(url)) => self.pending_open = Some(url),
                None => {}
            },
            Action::AudioPick(i) => {
                self.audio = Some(i);
                self.audio_open = false;
            }
            Action::SubtitlePick(v) => {
                self.subtitle = Some(v);
                self.subtitle_open = false;
            }
            Action::OpenSection => {
                self.close_pickers();
                self.section.open = true;
                self.section.error = None;
            }
            Action::SectionPick(i) => {
                self.section.open = false;
                let id = self.item.id.clone();
                admin_actions::move_to(cx, id, i);
            }
            Action::OpenPosition => {
                self.position.open = true;
                self.position.error = None;
                self.position.value = self
                    .place
                    .as_deref()
                    .and_then(|p| crate::tags::RfTags::parse(&self.current().tags).order.get(p).copied())
                    .map(i64::from)
                    .unwrap_or(0);
                self.audio_open = false;
                self.subtitle_open = false;
                self.section.open = false;
                self.version_open = false;
                self.focus = Focus { group: G_POS, index: 0 };
            }
            Action::PositionSave => {
                self.position.open = false;
                self.focus = Focus { group: G_BTN, index: self.position_button_index() };
                if let Some(place) = self.place.clone() {
                    let n = self.position.value.clamp(POS_MIN, POS_MAX) as u32;
                    let id = self.item.id.clone();
                    admin_actions::set_position(cx, id, place, n);
                }
            }
            Action::PositionCancel => {
                self.position.open = false;
                self.focus = Focus { group: G_BTN, index: self.position_button_index() };
            }
            Action::OpenStory => {
                self.close_pickers();
                let (n, era) = admin_actions::story_value(self.current());
                self.story.value = n;
                self.story.era = era;
                self.story.error = None;
                self.story.open = true;
                self.focus = Focus { group: G_STORY, index: 0 };
            }
            Action::StoryEra => {
                self.story.era = match self.story.era {
                    crate::tags::Era::Bby => crate::tags::Era::Aby,
                    crate::tags::Era::Aby => crate::tags::Era::Bby,
                };
            }
            Action::StorySave | Action::StoryClear => {
                self.story.open = false;
                self.focus = Focus { group: G_BTN, index: self.story_button_index() };
                let id = self.item.id.clone();
                let year = matches!(a, Action::StorySave).then(|| self.story.year());
                admin_actions::set_story(cx, id, year);
            }
            Action::StoryCancel => {
                self.story.open = false;
                self.focus = Focus { group: G_BTN, index: self.story_button_index() };
            }
            // G4 (P-TV-9): play the neighbouring episode from an episode page.
            Action::PlayPrevEp => {
                if let Some(ep) = self.prev_ep.clone() {
                    cx.push(player::screen(ep, PlayOptions::default()));
                }
            }
            Action::PlayNextEp => {
                if let Some(ep) = self.next_ep.clone() {
                    cx.push(player::screen(ep, PlayOptions::default()));
                }
            }
            // G4 (P-ITEM-17)
            Action::PlayChapter(s) => self.play_chapter(cx, s),
            Action::ToggleFavorite => self.toggle_favorite(cx),
            Action::OpenMore => cx.push(admin_actions::more_screen(self.current().clone(), self.is_admin)),
            Action::OpenAddTo => self.open_add_menu(cx),
            Action::AddPick(a) => self.pick_add(cx, a),
        }
    }

    fn enter_movie_episode(&mut self, cx: &mut Cx) -> bool {
        if self.focus.group == G_RETRY {
            self.spawn_loads(cx);
            self.error = None;
            return true;
        }
        // G8 (P-ITEM-11)
        if self.focus.group == G_LIST && self.version_open {
            let picked = self.version_entries().get(self.focus.index).map(|(_, i)| *i);
            if let Some(i) = picked {
                self.do_action(cx, Action::VersionPick(i));
            }
            let idx = self.movie_buttons().iter().position(|(_, a)| matches!(a, Action::OpenVersion)).unwrap_or(0);
            self.focus = Focus { group: G_BTN, index: idx };
            return true;
        }
        if self.focus.group == G_LIST && (self.audio_open || self.subtitle_open) {
            if self.audio_open {
                let picked = self.audio_entries().get(self.focus.index).map(|(_, i)| *i);
                if let Some(i) = picked {
                    self.audio = Some(i);
                }
                self.audio_open = false;
                self.focus = Focus { group: G_BTN, index: self.audio_button_index() };
            } else {
                let picked = self.subtitle_entries().get(self.focus.index).map(|(_, v)| *v);
                if let Some(v) = picked {
                    self.subtitle = Some(v);
                }
                self.subtitle_open = false;
                self.focus = Focus { group: G_BTN, index: self.subtitle_button_index() };
            }
            return true;
        }
        if self.focus.group == G_LIST && self.add_open {
            let act = self.add_entries().get(self.focus.index).map(|(_, a)| *a);
            if let Some(a) = act {
                self.pick_add(cx, a);
            }
            return true;
        }
        if self.focus.group == G_LIST && self.section.open {
            let choice = self.focus.index;
            self.section.open = false;
            let id = self.item.id.clone();
            admin_actions::move_to(cx, id, choice);
            self.focus = Focus { group: G_BTN, index: self.section_button_index() };
            return true;
        }
        // G4 (P-ITEM-17): Enter on a chapter plays from it.
        if self.focus.group == G_CHAPTERS {
            let secs = super::player::chapter_rows(&self.current().chapters).get(self.focus.index).map(|(s, _)| *s);
            if let Some(s) = secs {
                self.do_action(cx, Action::PlayChapter(s));
            }
            return true;
        }
        // RR6 (P-ITEM-16): Enter on a part plays the film from that part.
        if self.focus.group == G_PARTS {
            if self.focus.index < self.parts.len() {
                let opts = self.play_options(true);
                self.play_parts(cx, self.focus.index + 1, opts);
            }
            return true;
        }
        // G8 (P-ITEM-15): Enter on an extra plays it; (P-ITEM-14) on a similar item opens its page.
        if self.focus.group == G_SPECIALS {
            if let Some(it) = self.specials.get(self.focus.index).cloned() {
                cx.push(player::screen(it, PlayOptions { from_start: true, ..Default::default() }));
            }
            return true;
        }
        if self.focus.group == G_SIMILAR {
            if let Some(it) = self.similar.get(self.focus.index).cloned() {
                cx.push(super::item::screen(it));
            }
            return true;
        }
        if self.focus.group == G_INFO {
            return true;
        }
        if self.focus.group == G_PEOPLE {
            if let Some(p) = ordered_people(self.current()).get(self.focus.index).cloned() {
                cx.push(super::person::screen(p.id, p.name));
            }
            return true;
        }
        if self.focus.group == G_CHIPS {
            if let Some(c) = chips(self.current()).get(self.focus.index) {
                open_chip(cx, &c.target.clone());
            }
            return true;
        }
        if self.focus.group == G_LINKS {
            let links = external_links(&self.current().provider_ids, false);
            if let Some((_, url)) = links.get(self.focus.index) {
                self.pending_open = Some(url.clone());
            }
            return true;
        }
        if self.focus.group == G_POS {
            match self.focus.index {
                2 => self.do_action(cx, Action::PositionCancel),
                _ => self.do_action(cx, Action::PositionSave),
            }
            return true;
        }
        if self.focus.group == G_STORY {
            return self.story_key(cx, Key::Enter);
        }
        if self.focus.group != G_BTN {
            return false;
        }
        let action = self.movie_buttons().get(self.focus.index).map(|(_, a)| *a);
        if let Some(a) = action {
            self.do_action(cx, a);
        }
        true
    }

    fn enter_series(&mut self, cx: &mut Cx) -> bool {
        if self.focus.group == G_RETRY {
            self.spawn_loads(cx);
            self.error = None;
            return true;
        }
        if self.focus.group == G_BTN {
            self.play_next_episode(cx);
        } else if self.focus.group == G_LIST {
            if let Some(s) = self.seasons.get(self.focus.index).cloned() {
                cx.push(super::item::screen(s));
            }
        }
        true
    }

    fn enter_season(&mut self, cx: &mut Cx) -> bool {
        if self.focus.group == G_RETRY {
            self.spawn_loads(cx);
            self.error = None;
            return true;
        }
        if self.focus.group == G_LIST {
            if let Some(ep) = self.episodes.get(self.focus.index).cloned() {
                cx.push(super::item::screen(ep));
            }
        }
        true
    }

    fn row_focus(&self) -> Option<usize> {
        (self.focus.group == G_LIST).then_some(self.focus.index)
    }

    fn draw_people(&mut self, ui: &mut egui::Ui, cx: &mut Cx, people_xs: &mut Vec<f32>) -> Option<PersonRef> {
        let people = ordered_people(self.current());
        if people.is_empty() {
            return None;
        }
        let t = cx.theme;
        let pointer_moved = ui.input(|i| i.pointer.is_moving());
        let mut clicked = None;
        // D2: Apple's Cast & Crew row of round photos.
        at::section_title(ui, t, "Cast & crew");
        let focus = (self.focus.group == G_PEOPLE).then_some(self.focus.index);
        let rs = at::row(ui, t, "cast-crew", people.len(), at::person_h(t), focus, |ui, i| {
            let tx = at::person_tex(cx, ui, &people[i]);
            at::person_circle(ui, t, tx, &people[i].name, &role_label(&people[i]), focus == Some(i))
        });
        for (i, r) in rs.iter().enumerate() {
            people_xs.push(r.rect.center().x);
            if pointer_moved && r.hovered() {
                self.focus = Focus { group: G_PEOPLE, index: i };
            }
            if r.clicked() {
                clicked = Some(people[i].clone());
            }
        }
        clicked
    }

    /// P-ITEM-3: one row of genre / studio / director / writer link chips, each under its section name.
    fn draw_chips(&mut self, ui: &mut egui::Ui, cx: &mut Cx, chip_xs: &mut Vec<f32>) -> Option<ChipTarget> {
        let chips = chips(self.current());
        if chips.is_empty() {
            return None;
        }
        let t = cx.theme;
        let pointer_moved = ui.input(|i| i.pointer.is_moving());
        let mut clicked = None;
        ui.add_space(t.gap / 2.0);
        egui::ScrollArea::horizontal()
            .id_salt("item-chips")
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = t.gap / 2.0;
                    let mut last = "";
                    for (i, c) in chips.iter().enumerate() {
                        if c.section != last {
                            last = c.section;
                            ui.label(egui::RichText::new(c.section).font(t.font(t.small_size)).color(t.muted));
                        }
                        let focused = self.focus.group == G_CHIPS && self.focus.index == i;
                        let r = widgets::pill_button(ui, t, &c.label, focused, false);
                        chip_xs.push(r.rect.center().x);
                        if focused && crate::ui::shell::keyboard_focus(&r.ctx) {
                            r.scroll_to_me(None);
                        }
                        if pointer_moved && r.hovered() {
                            self.focus = Focus { group: G_CHIPS, index: i };
                        }
                        if r.clicked() {
                            clicked = Some(c.target.clone());
                        }
                    }
                });
            });
        clicked
    }

    fn draw_links(&mut self, ui: &mut egui::Ui, cx: &mut Cx, link_xs: &mut Vec<f32>) -> Option<String> {
        let links = external_links(&self.current().provider_ids, false);
        if links.is_empty() {
            return None;
        }
        let t = cx.theme;
        let pointer_moved = ui.input(|i| i.pointer.is_moving());
        let mut clicked = None;
        at::section_title(ui, t, "External links");
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = t.gap / 2.0;
            for (i, (label, url)) in links.iter().enumerate() {
                let focused = self.focus.group == G_LINKS && self.focus.index == i;
                let r = widgets::pill_button(ui, t, label, focused, false);
                link_xs.push(r.rect.center().x);
                if pointer_moved && r.hovered() {
                    self.focus = Focus { group: G_LINKS, index: i };
                }
                if r.clicked() {
                    clicked = Some(url.clone());
                }
            }
        });
        clicked
    }

    fn draw_movie_episode(
        &mut self,
        ui: &mut egui::Ui,
        cx: &mut Cx,
        list_xs: &mut Vec<f32>,
        pos_xs: &mut Vec<f32>,
        story_xs: &mut Vec<f32>,
    ) -> Option<Action> {
        let t = cx.theme;
        let mut act = None;
        // D2: the button row itself is drawn inside the hero (`page`); the pickers it opens stay here, below it.
        if self.audio_open {
            let entries = self.audio_entries();
            ui.add_space(t.gap / 2.0);
            for (i, (label, index)) in entries.iter().enumerate() {
                let focused = self.focus.group == G_LIST && self.focus.index == i;
                let r = parts::button(ui, t, label, focused, false);
                list_xs.push(r.rect.center().x);
                if r.hovered() {
                    self.focus = Focus { group: G_LIST, index: i };
                }
                if r.clicked() {
                    act = Some(Action::AudioPick(*index));
                }
            }
        } else if self.subtitle_open {
            let entries = self.subtitle_entries();
            ui.add_space(t.gap / 2.0);
            for (i, (label, val)) in entries.iter().enumerate() {
                let focused = self.focus.group == G_LIST && self.focus.index == i;
                let r = parts::button(ui, t, label, focused, false);
                list_xs.push(r.rect.center().x);
                if r.hovered() {
                    self.focus = Focus { group: G_LIST, index: i };
                }
                if r.clicked() {
                    act = Some(Action::SubtitlePick(*val));
                }
            }
        } else if self.add_open {
            ui.add_space(t.gap / 2.0);
            let loading = match self.add_mode {
                AddMode::Playlists => self.playlists.is_none(),
                AddMode::Collections => self.collections.is_none(),
                AddMode::Root => false,
            };
            let entries = self.add_entries();
            if loading {
                parts::muted(ui, t, "Loading…");
            } else if entries.is_empty() {
                parts::muted(ui, t, "Nothing here yet.");
            }
            for (i, (label, act_i)) in entries.iter().enumerate() {
                let focused = self.focus.group == G_LIST && self.focus.index == i;
                let r = parts::button(ui, t, label, focused, false);
                list_xs.push(r.rect.center().x);
                if r.hovered() {
                    self.focus = Focus { group: G_LIST, index: i };
                }
                if r.clicked() {
                    act = Some(Action::AddPick(*act_i));
                }
            }
        } else if self.version_open {
            let entries = self.version_entries();
            ui.add_space(t.gap / 2.0);
            for (i, (label, index)) in entries.iter().enumerate() {
                let focused = self.focus.group == G_LIST && self.focus.index == i;
                let r = parts::button(ui, t, label, focused, false);
                list_xs.push(r.rect.center().x);
                if r.hovered() {
                    self.focus = Focus { group: G_LIST, index: i };
                }
                if r.clicked() {
                    act = Some(Action::VersionPick(*index));
                }
            }
        } else if self.section.open {
            ui.add_space(t.gap / 2.0);
            for (i, (label, _)) in admin_actions::CHOICES.iter().enumerate() {
                let focused = self.focus.group == G_LIST && self.focus.index == i;
                let r = parts::button(ui, t, label, focused, false);
                list_xs.push(r.rect.center().x);
                if r.hovered() {
                    self.focus = Focus { group: G_LIST, index: i };
                }
                if r.clicked() {
                    act = Some(Action::SectionPick(i));
                }
            }
        } else if self.story.open {
            ui.add_space(t.gap / 2.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = t.gap / 2.0;
                let f = self.focus;
                let focus = |i: usize| f.group == G_STORY && f.index == i;
                let r = widgets::number_field(ui, t, self.story.value, focus(0));
                story_xs.push(r.rect.center().x);
                if r.hovered() {
                    self.focus = Focus { group: G_STORY, index: 0 };
                }
                let era = match self.story.era {
                    crate::tags::Era::Bby => "BBY",
                    crate::tags::Era::Aby => "ABY",
                };
                for (i, label, primary) in
                    [(1, era, false), (2, "Save", true), (3, "Clear", false), (4, "Cancel", false)]
                {
                    let r = parts::button(ui, t, label, focus(i), primary);
                    story_xs.push(r.rect.center().x);
                    if r.hovered() {
                        self.focus = Focus { group: G_STORY, index: i };
                    }
                    if r.clicked() {
                        act = Some(match i {
                            1 => Action::StoryEra,
                            2 => Action::StorySave,
                            3 => Action::StoryClear,
                            _ => Action::StoryCancel,
                        });
                    }
                }
            });
        } else if self.position.open {
            ui.add_space(t.gap / 2.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = t.gap / 2.0;
                let field_focused = self.focus.group == G_POS && self.focus.index == 0;
                let r = widgets::number_field(ui, t, self.position.value, field_focused);
                pos_xs.push(r.rect.center().x);
                if r.hovered() {
                    self.focus = Focus { group: G_POS, index: 0 };
                }
                let save_focused = self.focus.group == G_POS && self.focus.index == 1;
                let r = parts::button(ui, t, "Save", save_focused, true);
                pos_xs.push(r.rect.center().x);
                if r.hovered() {
                    self.focus = Focus { group: G_POS, index: 1 };
                }
                if r.clicked() {
                    act = Some(Action::PositionSave);
                }
                let cancel_focused = self.focus.group == G_POS && self.focus.index == 2;
                let r = parts::button(ui, t, "Cancel", cancel_focused, false);
                pos_xs.push(r.rect.center().x);
                if r.hovered() {
                    self.focus = Focus { group: G_POS, index: 2 };
                }
                if r.clicked() {
                    act = Some(Action::PositionCancel);
                }
            });
        }
        act
    }

    // G8 (P-ITEM-15 / P-ITEM-14): a card row of extras or similar items. Returns the clicked index.
    fn draw_card_row(
        &mut self,
        ui: &mut egui::Ui,
        cx: &mut Cx,
        group: usize,
        heading: &str,
        xs: &mut Vec<f32>,
    ) -> Option<usize> {
        let items = if group == G_SPECIALS { self.specials.clone() } else { self.similar.clone() };
        if items.is_empty() {
            return None;
        }
        let focus = (self.focus.group == group).then_some(self.focus.index);
        let pointer_moved = ui.input(|i| i.pointer.is_moving());
        let out = parts::row(
            ui,
            cx,
            &format!("g8-{group}"),
            heading,
            &items,
            if group == G_SPECIALS { CardKind::Thumb } else { CardKind::Poster },
            focus,
        );
        *xs = out.xs;
        if let (true, Some(i)) = (pointer_moved, out.hovered) {
            self.focus = Focus { group, index: i };
        }
        out.clicked
    }

    // RR6 (P-ITEM-16): "Additional parts" row; click or Enter plays the film from that part.
    fn draw_parts_row(&mut self, ui: &mut egui::Ui, cx: &mut Cx, xs: &mut Vec<f32>) -> Option<usize> {
        if self.parts.is_empty() {
            return None;
        }
        let focus = (self.focus.group == G_PARTS).then_some(self.focus.index);
        let pointer_moved = ui.input(|i| i.pointer.is_moving());
        let out = parts::row(ui, cx, "rr6-parts", "Additional parts", &self.parts, CardKind::Thumb, focus);
        *xs = out.xs;
        if let (true, Some(i)) = (pointer_moved, out.hovered) {
            self.focus = Focus { group: G_PARTS, index: i };
        }
        out.clicked
    }

    // G8 (P-ITEM-18): the technical facts of the chosen version. Path only for admins.
    fn draw_media_info(&mut self, ui: &mut egui::Ui, cx: &mut Cx, xs: &mut Vec<f32>) {
        let id = self.source().map(|m| m.id.clone());
        let info = self.media_info.iter().find(|m| Some(&m.id) == id.as_ref()).or_else(|| self.media_info.first());
        // D2: Apple's "Information" section — facts, people, then Audio / Subtitles / Format of the chosen version
        // (path only for admins). Each column is a focus stop, so the remote can scroll down to it.
        let cols = at::info_columns(self.current(), info, cx.admin);
        if cols.is_empty() {
            return;
        }
        let t = cx.theme;
        let pointer_moved = ui.input(|i| i.pointer.is_moving());
        at::section_title(ui, t, "Information");
        let focus = (self.focus.group == G_INFO).then_some(self.focus.index);
        for (i, r) in at::info_section(ui, t, &cols, focus).iter().enumerate() {
            xs.push(r.rect.center().x);
            if pointer_moved && r.hovered() {
                self.focus = Focus { group: G_INFO, index: i };
            }
        }
    }

    // G4 / RR6 (P-ITEM-17): a "Chapters" row on movie/episode pages: a 16:9 still per chapter (the server's chapter
    // image) with time + name under it; Enter plays from the chapter. Always drawn so the "No chapters" state is
    // visible in the render shots.
    fn draw_chapters(&mut self, ui: &mut egui::Ui, cx: &mut Cx, chapter_xs: &mut Vec<f32>) -> Option<Action> {
        let t = cx.theme;
        let rows = super::player::chapter_rows(&self.current().chapters);
        at::section_title(ui, t, "Chapters");
        if rows.is_empty() {
            parts::muted(ui, t, "No chapters");
            return None;
        }
        // same order as `chapter_rows` (stable sort by start); the original index is what the image URL wants
        let mut order: Vec<(usize, crate::api::Chapter)> =
            self.current().chapters.iter().cloned().enumerate().collect();
        order.sort_by_key(|(_, c)| c.start_position_ticks);
        let id = self.current().id.clone();
        let w = image_width_bucket(t.thumb_w, ui.ctx().pixels_per_point());
        let pointer_moved = ui.input(|i| i.pointer.is_moving());
        let focus = (self.focus.group == G_CHAPTERS).then_some(self.focus.index);
        let h = t.thumb_h + t.card_title_gap + t.body_size * 1.3;
        let rs = at::row(ui, t, "chapters", rows.len(), h, focus, |ui, i| {
            let (orig, ch) = &order[i];
            let tex = ch
                .image_tag
                .as_deref()
                .and_then(|tag| cx.client.as_ref().map(|c| c.chapter_image_url(&id, *orig, tag, w)))
                .and_then(|u| cx.images.get(&u));
            at::still_card(ui, t, tex, &rows[i].1, focus == Some(i))
        });
        let mut act = None;
        for (i, r) in rs.iter().enumerate() {
            chapter_xs.push(r.rect.center().x);
            if pointer_moved && r.hovered() {
                self.focus = Focus { group: G_CHAPTERS, index: i };
            }
            if r.clicked() {
                act = Some(Action::PlayChapter(rows[i].0));
            }
        }
        act
    }

    fn draw_series(
        &mut self,
        ui: &mut egui::Ui,
        cx: &mut Cx,
        play_xs: &mut Vec<f32>,
        row_xs: &mut Vec<f32>,
    ) -> (Option<Action>, Option<Item>) {
        let t = cx.theme;
        let mut act = None;
        let mut push_item = None;

        let focused = self.focus.group == G_BTN && self.focus.index == 0;
        let r = parts::button(ui, t, "Play next episode", focused, true);
        play_xs.push(r.rect.center().x);
        if r.hovered() {
            self.focus = Focus { group: G_BTN, index: 0 };
        }
        if r.clicked() {
            act = Some(Action::PlayNext);
        }

        if !self.seasons.is_empty() {
            let out = parts::row(ui, cx, "seasons", "Seasons", &self.seasons, CardKind::Poster, self.row_focus());
            *row_xs = out.xs;
            if let Some(i) = out.hovered {
                self.focus = Focus { group: G_LIST, index: i };
            }
            if let Some(i) = out.clicked {
                push_item = self.seasons.get(i).cloned();
            }
        }
        (act, push_item)
    }

    fn draw_season(&mut self, ui: &mut egui::Ui, cx: &mut Cx, row_xs: &mut Vec<f32>) -> Option<Item> {
        let t = cx.theme;
        let mut push_item = None;
        let ppp = ui.ctx().pixels_per_point();
        let thumb_w = t.thumb_w * 0.5;
        let thumb_h = t.thumb_h * 0.5;
        let row_h = thumb_h + t.gap / 2.0;
        ui.add_space(t.gap / 2.0);
        egui::ScrollArea::vertical().id_salt("season-episodes").auto_shrink([false, false]).show(ui, |ui| {
            for (i, ep) in self.episodes.iter().enumerate() {
                let focused = self.focus.group == G_LIST && self.focus.index == i;
                let (rect, resp) =
                    ui.allocate_exact_size(egui::vec2(ui.available_width(), row_h), egui::Sense::click());
                if ui.is_rect_visible(rect) {
                    let thumb_rect = egui::Rect::from_min_size(rect.min, egui::vec2(thumb_w, thumb_h));
                    let w = image_width_bucket(thumb_w, ppp);
                    let url = cx.client.as_ref().and_then(|c| c.image_url(ep, ImageKind::Primary, w));
                    match url.as_deref().and_then(|u| cx.images.get(u)) {
                        Some(tex) => {
                            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                            ui.painter().image(tex, thumb_rect, uv, egui::Color32::WHITE);
                        }
                        None => {
                            ui.painter().rect_filled(thumb_rect, t.radius, t.skeleton);
                        }
                    }
                    if let Some(f) = parts::resume_fraction(ep) {
                        parts::progress_bar(ui, t, thumb_rect, f);
                    }
                    if focused {
                        parts::focus_ring(ui, t, thumb_rect);
                    }
                    let title = format!("E{} · {}", ep.index_number.unwrap_or(0), ep.name);
                    let x = thumb_rect.right() + t.gap / 2.0;
                    let w_text = rect.right() - x - t.gap;
                    parts::label_line(
                        ui,
                        t,
                        egui::pos2(x, rect.top() + t.small_size),
                        w_text,
                        &title,
                        t.body_size,
                        t.text,
                    );
                    let runtime = ep.run_time_ticks.map(|rt| parts::clock(ticks_to_secs(rt))).unwrap_or_default();
                    let played = ep.user_data.as_ref().map(|u| u.played).unwrap_or(false);
                    let sub = if played { format!("{runtime}  \u{2713}") } else { runtime };
                    parts::label_line(
                        ui,
                        t,
                        egui::pos2(x, rect.top() + t.small_size + t.body_size + 6.0),
                        w_text,
                        &sub,
                        t.small_size,
                        t.muted,
                    );
                }
                row_xs.push(rect.center().x);
                if resp.hovered() {
                    self.focus = Focus { group: G_LIST, index: i };
                }
                if focused && crate::ui::shell::keyboard_focus(&resp.ctx) {
                    resp.scroll_to_me(None);
                }
                if resp.clicked() {
                    push_item = Some(ep.clone());
                }
            }
        });
        push_item
    }
}

/// G8 / P-ITEM-8: a local trailer wins over a remote one; remote URLs with nothing in them are skipped.
pub fn trailer_target(local: &[Item], remote: &[MediaUrl]) -> Option<TrailerTarget> {
    if let Some(it) = local.first() {
        return Some(TrailerTarget::Local(Box::new(it.clone())));
    }
    remote.iter().map(|r| r.url.trim()).find(|u| !u.is_empty()).map(|u| TrailerTarget::Remote(u.to_string()))
}

/// G8 / P-ITEM-11: the picker label of version `i`: the server's source name when the media info has loaded,
/// else "Version N".
pub fn version_label(i: usize, src: Option<&crate::api::MediaSource>, infos: &[SourceInfo]) -> String {
    let named = src.and_then(|s| infos.iter().find(|m| m.id == s.id)).or_else(|| infos.get(i));
    if let Some(n) = named.and_then(|m| m.name.as_deref()).map(str::trim).filter(|n| !n.is_empty()) {
        return n.to_string();
    }
    format!("Version {}", i + 1)
}

/// G8 / P-ITEM-18: "1.4 GB", "820 MB".
pub fn format_size(bytes: i64) -> String {
    let b = bytes.max(0) as f64;
    if b >= 1e9 {
        format!("{:.1} GB", b / 1e9)
    } else {
        format!("{:.0} MB", b / 1e6)
    }
}

/// G8 / P-ITEM-18: "8.2 Mbps", "640 kbps".
pub fn format_bitrate(bps: i64) -> String {
    let b = bps.max(0) as f64;
    if b >= 1e6 {
        format!("{:.1} Mbps", b / 1e6)
    } else {
        format!("{:.0} kbps", b / 1e3)
    }
}

/// G8 / P-ITEM-18: label/value lines of the media info panel for one source. The file path is for admins only.
pub fn media_info_lines(src: &SourceInfo, admin: bool) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = vec![];
    if let Some(c) = src.container.as_deref().filter(|c| !c.is_empty()) {
        v.push(("Container".into(), c.to_string()));
    }
    for s in src.media_streams.iter().filter(|s| s.kind == StreamKind::Video) {
        let mut parts = vec![];
        if let Some(c) = s.codec.as_deref().filter(|c| !c.is_empty()) {
            parts.push(c.to_string());
        }
        if let (Some(w), Some(h)) = (s.width, s.height) {
            parts.push(format!("{w}×{h}"));
        }
        if let Some(b) = s.bit_rate {
            parts.push(format_bitrate(b));
        }
        if !parts.is_empty() {
            v.push(("Video".into(), parts.join(" · ")));
        }
    }
    for s in src.media_streams.iter().filter(|s| s.kind == StreamKind::Audio) {
        let text = s
            .display_title
            .clone()
            .filter(|t| !t.is_empty())
            .or_else(|| s.language.clone())
            .or_else(|| s.codec.clone())
            .unwrap_or_else(|| format!("Track {}", s.index));
        v.push(("Audio".into(), text));
    }
    let subs: Vec<String> = src
        .media_streams
        .iter()
        .filter(|s| s.kind == StreamKind::Subtitle)
        .map(|s| {
            s.display_title
                .clone()
                .filter(|t| !t.is_empty())
                .or_else(|| s.language.clone())
                .unwrap_or_else(|| format!("Subtitle {}", s.index))
        })
        .collect();
    if !subs.is_empty() {
        v.push(("Subtitles".into(), subs.join(", ")));
    }
    if let Some(b) = src.bitrate {
        v.push(("Bitrate".into(), format_bitrate(b)));
    }
    if let Some(sz) = src.size {
        v.push(("Size".into(), format_size(sz)));
    }
    if admin {
        if let Some(p) = src.path.as_deref().filter(|p| !p.is_empty()) {
            v.push(("Path".into(), p.to_string()));
        }
    }
    v
}

/// P-ITEM-3: where a genre / studio / director / writer chip goes.
#[derive(Clone, Debug, PartialEq)]
pub enum ChipTarget {
    Genre(String),
    Studio { id: String, name: String },
    Person { id: String, name: String },
}

/// P-ITEM-3: one chip: the section it sits under ("Genres", "Studios", "Director", "Writers"), its label, its target.
#[derive(Clone, Debug, PartialEq)]
pub struct Chip {
    pub section: &'static str,
    pub label: String,
    pub target: ChipTarget,
}

/// P-ITEM-3: the focusable link chips of an item page, in the order genres · studios · directors · writers.
pub fn chips(item: &Item) -> Vec<Chip> {
    let mut v: Vec<Chip> = item
        .genres
        .iter()
        .map(|g| Chip { section: "Genres", label: g.clone(), target: ChipTarget::Genre(g.clone()) })
        .collect();
    v.extend(item.studios.iter().filter(|s| !s.id.is_empty()).map(|s| Chip {
        section: "Studios",
        label: s.name.clone(),
        target: ChipTarget::Studio { id: s.id.clone(), name: s.name.clone() },
    }));
    for (kind, one, many) in [("Director", "Director", "Directors"), ("Writer", "Writer", "Writers")] {
        let who: Vec<&PersonRef> = item.people.iter().filter(|p| p.kind == kind && !p.id.is_empty()).collect();
        let section = if who.len() == 1 { one } else { many };
        v.extend(who.into_iter().map(|p| Chip {
            section,
            label: p.name.clone(),
            target: ChipTarget::Person { id: p.id.clone(), name: p.name.clone() },
        }));
    }
    v
}

fn open_chip(cx: &mut Cx, target: &ChipTarget) {
    match target {
        ChipTarget::Genre(g) => cx.push(super::browse::genre(g)),
        ChipTarget::Studio { id, name } => cx.push(super::browse::studio(id, name)),
        ChipTarget::Person { id, name } => cx.push(super::person::screen(id.clone(), name.clone())),
    }
}

/// P-ITEM-19: external links built from `ProviderIds`. Only the ids the server supplied are returned, in
/// IMDb · TMDB · TVDB order. TMDB is `tv` for series, `movie` otherwise.
pub fn external_links(
    provider_ids: &std::collections::HashMap<String, String>,
    is_series: bool,
) -> Vec<(String, String)> {
    let mut links = vec![];
    if let Some(id) = provider_ids.get("Imdb") {
        links.push(("IMDb".to_string(), format!("https://www.imdb.com/title/{id}/")));
    }
    if let Some(id) = provider_ids.get("Tmdb") {
        let kind = if is_series { "tv" } else { "movie" };
        links.push(("TMDB".to_string(), format!("https://www.themoviedb.org/{kind}/{id}")));
    }
    if let Some(id) = provider_ids.get("Tvdb") {
        links.push(("TVDB".to_string(), format!("https://thetvdb.com/?tab=series&id={id}")));
    }
    links
}

/// The role shown under a person's name: their character for actors, the crew role otherwise.
fn role_label(p: &PersonRef) -> String {
    if let Some(r) = p.role.as_deref() {
        let r = r.trim();
        if !r.is_empty() {
            return r.to_string();
        }
    }
    match p.kind.as_str() {
        "GuestStar" => "Guest star".to_string(),
        other => other.to_string(),
    }
}

fn kind_rank(kind: &str) -> usize {
    match kind {
        "Actor" => 0,
        "GuestStar" => 1,
        "Director" => 2,
        "Writer" => 3,
        _ => 4,
    }
}

/// Actors first, then guest stars, directors, writers, everyone else (P-ITEM-13, P-TV-8).
fn ordered_people(item: &Item) -> Vec<PersonRef> {
    let mut v = item.people.clone();
    v.sort_by_key(|p| kind_rank(&p.kind));
    v
}

impl ItemScreen {
    fn page(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        if !self.started {
            self.started = true;
            self.spawn_loads(cx);
        }
        self.init_tracks();
        self.is_admin = cx.admin;
        let t = cx.theme;
        let mut more_x: Option<f32> = None;
        let mut more_hover = false;
        let mut more_click = false;

        // D2: full-bleed hero in the Apple TV look; the text column and (films / episodes) the action row sit on its
        // bottom edge; an expanded overview grows the page below it.
        let mut hero_xs: Vec<f32> = vec![];
        let mut hero_act: Option<Action> = None;
        {
            let item = self.current().clone();
            // G12 / P-USER-2: backdrops can be switched off in Settings.
            let backdrop = if cx.settings.backdrops {
                at::tex(cx, ui, &item, ImageKind::Backdrop, ui.ctx().screen_rect().width())
            } else {
                None
            };
            let art = HeroArt { backdrop, logo: at::logo(cx, ui, &item) };
            let rect = at::hero(ui, t, art, t.backdrop_h);
            let hero_focus = matches!(self.focus.group, G_BTN | G_MORE).then_some(0);
            if at::focus_moved(ui, egui::Id::new("item-hero"), hero_focus) {
                ui.scroll_to_rect(rect.with_max_y(rect.top() + t.gap), None);
            }
            let btns =
                if matches!(self.kind, ItemKind::Movie | ItemKind::Episode) { self.action_btns() } else { vec![] };
            let years = crate::story::year_line(&item, cx.settings.show_release_year, cx.settings.show_story_year);
            let mut meta = at::meta_parts(&item, &years);
            if item.kind == ItemKind::Episode {
                // the episode line already says which show; keep the facts short
                meta.retain(|m| !item.genres.iter().any(|g| m.contains(g.as_str())));
            }
            let facts: Vec<at::StreamFacts> = match self.media_info.first() {
                Some(src) => src.media_streams.iter().map(at::StreamFacts::from).collect(),
                None => item.media_streams.iter().map(at::StreamFacts::from).collect(),
            };
            let badges = at::badges(&facts);
            let moving = ui.input(|i| i.pointer.is_moving());
            at::hero_column(ui, t, rect, "item-hero", |col| {
                if let Some(line) = self.above_line(&item) {
                    at::kicker(col, t, &line);
                    col.add_space(t.gap / 4.0);
                }
                at::hero_title(col, t, &item.name, art.logo);
                if let Some(tagline) = item.taglines.first().map(|s| s.trim()).filter(|s| !s.is_empty()) {
                    col.add_space(t.gap / 4.0);
                    col.label(egui::RichText::new(tagline).font(t.font(t.body_size)).color(t.muted).italics());
                }
                col.add_space(t.gap / 2.0);
                at::meta_row(col, t, &meta, (item.community_rating, item.critic_rating), &badges);
                if let Some(ov) = item.overview.as_deref().map(str::trim).filter(|o| !o.is_empty()) {
                    col.add_space(t.gap / 2.0);
                    let long = at::overview_rows(col, t, ov, t.hero_text_w) > OVERVIEW_LINES;
                    at::overview(col, t, ov, self.overview_open || !long, OVERVIEW_LINES);
                    if long {
                        col.add_space(t.gap / 4.0);
                        let label = if self.overview_open { "Less" } else { "More" };
                        let r = at::text_button(col, t, label, self.focus.group == G_MORE);
                        more_x = Some(r.rect.center().x);
                        more_hover = r.hovered();
                        more_click = r.clicked();
                    }
                }
                if !self.in_collections.is_empty() {
                    let names = self.in_collections.iter().map(|c| c.name.as_str()).collect::<Vec<_>>().join(", ");
                    col.add_space(t.gap / 4.0);
                    col.add(
                        egui::Label::new(
                            egui::RichText::new(format!("Collections  {names}"))
                                .font(t.font(t.small_size))
                                .color(t.muted),
                        )
                        .wrap(),
                    );
                }
                if let Some(note) = &self.add_note {
                    col.add_space(t.gap / 4.0);
                    col.add(
                        egui::Label::new(egui::RichText::new(note).font(t.font(t.small_size)).color(t.text)).wrap(),
                    );
                }
                if !btns.is_empty() {
                    col.add_space(t.gap);
                    let shown: Vec<ActionBtn> = btns.iter().map(|(b, _)| b.clone()).collect();
                    let focus = (self.focus.group == G_BTN).then_some(self.focus.index);
                    for (i, r) in at::action_row(col, t, &shown, focus).iter().enumerate() {
                        hero_xs.push(r.rect.center().x);
                        if r.hovered() && moving {
                            self.focus = Focus { group: G_BTN, index: i };
                        }
                        if r.clicked() {
                            hero_act = Some(btns[i].1);
                        }
                    }
                }
            });
        }

        if more_hover && ui.input(|i| i.pointer.is_moving()) {
            self.focus = Focus { group: G_MORE, index: 0 };
        }
        if more_click {
            self.overview_open = !self.overview_open;
        }

        ui.add_space(t.gap);
        let mut act: Option<Action> = hero_act;
        let mut push_item: Option<Item> = None;
        let mut btn_xs: Vec<f32> = hero_xs;
        let mut list_xs: Vec<f32> = vec![];
        let mut row_xs: Vec<f32> = vec![];
        let mut people_xs: Vec<f32> = vec![];
        let mut chip_xs: Vec<f32> = vec![];
        let mut link_xs: Vec<f32> = vec![];
        let mut push_person: Option<PersonRef> = None;
        let mut open_chip_target: Option<ChipTarget> = None;
        let mut open_url: Option<String> = None;
        let mut chapter_xs: Vec<f32> = vec![];
        let mut special_xs: Vec<f32> = vec![];
        let mut similar_xs: Vec<f32> = vec![];
        let mut info_xs: Vec<f32> = vec![];
        let mut play_special: Option<Item> = None;
        let mut play_part: Option<usize> = None;
        let mut part_xs: Vec<f32> = vec![];
        let mut pos_xs: Vec<f32> = vec![];
        let mut story_xs: Vec<f32> = vec![];

        match self.kind {
            ItemKind::Movie | ItemKind::Episode => {
                if let Some(a) = self.draw_movie_episode(ui, cx, &mut list_xs, &mut pos_xs, &mut story_xs) {
                    act = Some(a);
                }
                if !self.picker_open() {
                    open_chip_target = self.draw_chips(ui, cx, &mut chip_xs);
                }
                // G4 (P-ITEM-17): a Chapters row below the buttons (hidden while a picker is open).
                if !self.picker_open() {
                    let a = self.draw_chapters(ui, cx, &mut chapter_xs);
                    if act.is_none() {
                        act = a;
                    }
                }
                // RR6 (P-ITEM-16): the other parts of a multi-part film.
                if !self.picker_open() {
                    play_part = self.draw_parts_row(ui, cx, &mut part_xs);
                }
                push_person = self.draw_people(ui, cx, &mut people_xs);
                // G8: extras, more like this, media info.
                if let Some(i) = self.draw_card_row(ui, cx, G_SPECIALS, "Special features", &mut special_xs) {
                    play_special = self.specials.get(i).cloned();
                }
                if let Some(i) = self.draw_card_row(ui, cx, G_SIMILAR, "More like this", &mut similar_xs) {
                    if push_item.is_none() {
                        push_item = self.similar.get(i).cloned();
                    }
                }
                self.draw_media_info(ui, cx, &mut info_xs);
                open_url = self.draw_links(ui, cx, &mut link_xs);
            }
            ItemKind::Series => {
                let (a, pushed) = self.draw_series(ui, cx, &mut btn_xs, &mut row_xs);
                act = a;
                push_item = pushed;
            }
            ItemKind::Season => {
                push_item = self.draw_season(ui, cx, &mut row_xs);
            }
            _ => {}
        }

        let mut retry_xs: Vec<f32> = vec![];
        if let Some(e) = self.error.clone() {
            ui.add_space(t.gap / 2.0);
            parts::error_line(ui, t, &e);
            let focused = self.focus.group == G_RETRY;
            let r = parts::button(ui, t, "Retry", focused, false);
            retry_xs.push(r.rect.center().x);
            if r.hovered() {
                self.focus = Focus { group: G_RETRY, index: 0 };
            }
            if r.clicked() {
                self.spawn_loads(cx);
                self.error = None;
            }
        }

        let mut groups = vec![Group::default(); 14];
        match self.kind {
            ItemKind::Movie | ItemKind::Episode => {
                let n = btn_xs.len();
                groups[G_BTN] = Group { len: n, cols: n, xs: btn_xs };
                if self.story.open {
                    groups[G_STORY] = Group { len: story_xs.len(), cols: story_xs.len(), xs: story_xs };
                } else if self.position.open {
                    groups[G_POS] = Group { len: pos_xs.len(), cols: pos_xs.len(), xs: pos_xs };
                } else if self.picker_open() {
                    groups[G_LIST] = Group { len: list_xs.len(), cols: 1, xs: list_xs };
                } else {
                    // G4 (P-ITEM-17)
                    groups[G_CHAPTERS] = Group { len: chapter_xs.len(), cols: chapter_xs.len(), xs: chapter_xs };
                }
                groups[G_CHIPS] = Group { len: chip_xs.len(), cols: chip_xs.len(), xs: chip_xs };
                groups[G_PARTS] = Group { len: part_xs.len(), cols: part_xs.len(), xs: part_xs };
                groups[G_PEOPLE] = Group { len: people_xs.len(), cols: people_xs.len(), xs: people_xs };
                groups[G_SPECIALS] = Group { len: special_xs.len(), cols: special_xs.len(), xs: special_xs };
                groups[G_SIMILAR] = Group { len: similar_xs.len(), cols: similar_xs.len(), xs: similar_xs };
                groups[G_INFO] = Group { len: info_xs.len(), cols: info_xs.len(), xs: info_xs };
                groups[G_LINKS] = Group { len: link_xs.len(), cols: link_xs.len(), xs: link_xs };
            }
            ItemKind::Series => {
                groups[G_BTN] = Group { len: 1, cols: 1, xs: btn_xs };
                groups[G_LIST] = Group { len: row_xs.len(), cols: row_xs.len(), xs: row_xs };
            }
            ItemKind::Season => {
                groups[G_LIST] = Group { len: row_xs.len(), cols: 1, xs: row_xs };
            }
            _ => {}
        }
        if let Some(x) = more_x {
            groups[G_MORE] = Group { len: 1, cols: 1, xs: vec![x] };
        }
        if !retry_xs.is_empty() {
            groups[G_RETRY] = Group { len: 1, cols: 1, xs: retry_xs };
        }
        if let Some(e) = self.section.error.clone() {
            ui.add_space(t.gap / 2.0);
            parts::error_line(ui, t, &e);
        }
        if let Some(e) = self.position.error.clone() {
            ui.add_space(t.gap / 2.0);
            parts::error_line(ui, t, &e);
        }
        if let Some(e) = self.story.error.clone() {
            ui.add_space(t.gap / 2.0);
            parts::error_line(ui, t, &e);
        }
        self.groups = groups;
        self.focus = move_focus(&self.groups, self.focus, Key::Enter);

        if let Some(a) = act {
            self.do_action(cx, a);
        }
        if let Some(it) = push_item {
            cx.push(super::item::screen(it));
        }
        if let Some(i) = play_part {
            let opts = self.play_options(true);
            self.play_parts(cx, i + 1, opts);
        }
        if let Some(it) = play_special {
            cx.push(player::screen(it, PlayOptions { from_start: true, ..Default::default() }));
        }
        if let Some(p) = push_person {
            cx.push(super::person::screen(p.id, p.name));
        }
        if let Some(target) = open_chip_target {
            open_chip(cx, &target);
        }
        if let Some(url) = open_url.or_else(|| self.pending_open.take()) {
            ui.ctx().open_url(egui::OpenUrl::new_tab(url));
        }
    }
}

impl Screen for ItemScreen {
    fn name(&self) -> &'static str {
        match self.item.kind {
            ItemKind::Movie => "movie",
            ItemKind::Series => "series",
            ItemKind::Season => "season",
            ItemKind::Episode => "episode",
            _ => "item",
        }
    }

    /// NAV: this screen's part of the toolbar breadcrumb.
    fn crumbs(&self) -> Vec<String> {
        vec![self.item.name.clone()]
    }

    fn on_key(&mut self, cx: &mut Cx, key: Key) -> bool {
        if self.position.open {
            return self.position_key(cx, key);
        }
        if self.story.open {
            return self.story_key(cx, key);
        }
        match key {
            Key::Enter if self.focus.group == G_MORE => {
                self.overview_open = !self.overview_open;
                true
            }
            Key::Enter => match self.kind {
                ItemKind::Movie | ItemKind::Episode => self.enter_movie_episode(cx),
                ItemKind::Series => self.enter_series(cx),
                ItemKind::Season => self.enter_season(cx),
                _ => false,
            },
            Key::Back => {
                if self.add_open && self.add_mode != AddMode::Root {
                    self.add_mode = AddMode::Root;
                    self.focus = Focus { group: G_LIST, index: 0 };
                    true
                } else if self.picker_open() {
                    self.close_pickers();
                    true
                } else {
                    false
                }
            }
            Key::Up | Key::Down | Key::Left | Key::Right => {
                self.focus = move_focus(&self.groups, self.focus, key);
                true
            }
            _ => false,
        }
    }

    fn on_done(&mut self, cx: &mut Cx, tag: &'static str, payload: Box<dyn std::any::Any + Send>) {
        match tag {
            "item" => {
                self.loading = false;
                let Ok(r) = payload.downcast::<Result<Item>>() else { return };
                match *r {
                    Ok(it) => {
                        self.full = Some(it);
                        self.error = None;
                    }
                    Err(ApiError::Unauthorized) => cx.signed_out(),
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
            "seasons" => {
                let Ok(r) = payload.downcast::<Result<Vec<Item>>>() else { return };
                match *r {
                    Ok(v) => self.seasons = v,
                    Err(ApiError::Unauthorized) => cx.signed_out(),
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
            "episodes" => {
                let Ok(r) = payload.downcast::<Result<Vec<Item>>>() else { return };
                match *r {
                    Ok(v) => self.episodes = v,
                    Err(ApiError::Unauthorized) => cx.signed_out(),
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
            "nextup" => {
                let Ok(r) = payload.downcast::<Result<Vec<Item>>>() else { return };
                match *r {
                    Ok(v) => self.next_up = v,
                    Err(ApiError::Unauthorized) => cx.signed_out(),
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
            "played" => {
                let Ok(r) = payload.downcast::<Result<()>>() else { return };
                match *r {
                    Ok(()) => self.refresh(cx),
                    Err(ApiError::Unauthorized) => cx.signed_out(),
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
            // G4 (P-TV-9)
            "prev-ep" => {
                let Ok(r) = payload.downcast::<Result<Option<Item>>>() else { return };
                match *r {
                    Ok(v) => self.prev_ep = v,
                    Err(ApiError::Unauthorized) => cx.signed_out(),
                    Err(_) => self.prev_ep = None,
                }
            }
            "next-ep" => {
                let Ok(r) = payload.downcast::<Result<Option<Item>>>() else { return };
                match *r {
                    Ok(v) => self.next_ep = v,
                    Err(ApiError::Unauthorized) => cx.signed_out(),
                    Err(_) => self.next_ep = None,
                }
            }
            "favorite" => {
                let Ok(r) = payload.downcast::<Result<()>>() else { return };
                match *r {
                    Ok(()) => self.refresh(cx),
                    Err(ApiError::Unauthorized) => cx.signed_out(),
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
            // G7
            "playlists" => {
                let Ok(r) = payload.downcast::<Result<Vec<Item>>>() else { return };
                match *r {
                    Ok(v) => self.playlists = Some(v),
                    Err(ApiError::Unauthorized) => cx.signed_out(),
                    Err(e) => {
                        self.playlists = Some(vec![]);
                        self.add_note = Some(e.to_string());
                    }
                }
            }
            "collections" => {
                let Ok(r) = payload.downcast::<Result<Vec<Item>>>() else { return };
                match *r {
                    Ok(v) => self.collections = Some(v),
                    Err(ApiError::Unauthorized) => cx.signed_out(),
                    Err(e) => {
                        self.collections = Some(vec![]);
                        self.add_note = Some(e.to_string());
                    }
                }
            }
            "in-collections" => {
                let Ok(r) = payload.downcast::<Result<Vec<Item>>>() else { return };
                match *r {
                    Ok(v) => self.in_collections = v,
                    Err(ApiError::Unauthorized) => cx.signed_out(),
                    Err(_) => self.in_collections = vec![],
                }
            }
            "add-done" => {
                let Ok(r) = payload.downcast::<Result<String>>() else { return };
                match *r {
                    Ok(msg) => {
                        self.add_note = Some(msg);
                        self.load_in_collections(cx);
                    }
                    Err(ApiError::Unauthorized) => cx.signed_out(),
                    Err(e) => self.add_note = Some(e.to_string()),
                }
            }
            // G8: the extras. A failed extra is left out quietly; only Unauthorized matters.
            "trailers" | "specials" | "similar" | "parts" => {
                let Ok(r) = payload.downcast::<Result<Vec<Item>>>() else { return };
                match (*r, tag) {
                    (Ok(v), "trailers") => self.trailers = v,
                    (Ok(v), "specials") => self.specials = v,
                    (Ok(v), "parts") => self.parts = v,
                    (Ok(v), _) => self.similar = v,
                    (Err(ApiError::Unauthorized), _) => cx.signed_out(),
                    (Err(_), _) => {}
                }
            }
            "media-info" => {
                let Ok(r) = payload.downcast::<Result<Vec<SourceInfo>>>() else { return };
                match *r {
                    Ok(v) => self.media_info = v,
                    Err(ApiError::Unauthorized) => cx.signed_out(),
                    Err(_) => {}
                }
            }
            admin_actions::TAG => {
                if self.section.on_done(cx, payload) {
                    self.refresh(cx);
                }
            }
            admin_actions::STORY_TAG => {
                if self.story.on_done(cx, payload) {
                    self.refresh(cx);
                }
            }
            admin_actions::ORDER_TAG => {
                if self.position.on_done(cx, payload) {
                    self.refresh(cx);
                }
            }
            _ => {}
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        // G8: the movie / episode page is taller than the screen once the extra rows are in, so it scrolls.
        if matches!(self.kind, ItemKind::Movie | ItemKind::Episode) {
            egui::ScrollArea::vertical()
                .id_salt("item-page")
                .auto_shrink([false, false])
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(ui, |ui| self.page(ui, cx));
        } else {
            self.page(ui, cx);
        }
    }

    fn on_resume(&mut self, cx: &mut Cx) {
        self.refresh(cx);
    }
}

#[cfg(test)]
mod tests {
    use super::external_links;
    use std::collections::HashMap;

    #[test]
    fn external_links_movie() {
        let mut ids = HashMap::new();
        ids.insert("Imdb".to_string(), "tt0119116".to_string());
        ids.insert("Tmdb".to_string(), "10528".to_string());
        ids.insert("Tvdb".to_string(), "76290".to_string());
        assert_eq!(
            external_links(&ids, false),
            vec![
                ("IMDb".to_string(), "https://www.imdb.com/title/tt0119116/".to_string()),
                ("TMDB".to_string(), "https://www.themoviedb.org/movie/10528".to_string()),
                ("TVDB".to_string(), "https://thetvdb.com/?tab=series&id=76290".to_string()),
            ]
        );
    }

    #[test]
    fn external_links_series_uses_tv() {
        let mut ids = HashMap::new();
        ids.insert("Tmdb".to_string(), "10528".to_string());
        let links = external_links(&ids, true);
        assert_eq!(links, vec![("TMDB".to_string(), "https://www.themoviedb.org/tv/10528".to_string())]);
    }

    #[test]
    fn external_links_empty_when_no_ids() {
        assert!(external_links(&HashMap::new(), false).is_empty());
        let mut ids = HashMap::new();
        ids.insert("MusicBrainzAlbum".to_string(), "x".to_string());
        assert!(external_links(&ids, false).is_empty());
    }

    // G8: fixtures (the throwaway server has no trailers, extras or versions).
    use super::*;
    use crate::api::{MediaSource, StreamKind};

    fn stream(kind: StreamKind, index: i32) -> crate::api::StreamInfo {
        crate::api::StreamInfo { index, kind, ..Default::default() }
    }

    fn info() -> SourceInfo {
        SourceInfo {
            id: "s1".into(),
            name: Some("Ember Rising - 4K".into()),
            container: Some("mkv".into()),
            path: Some("/media/films/ember.mkv".into()),
            size: Some(1_400_000_000),
            bitrate: Some(9_000_000),
            media_streams: vec![
                crate::api::StreamInfo {
                    codec: Some("hevc".into()),
                    width: Some(3840),
                    height: Some(2160),
                    bit_rate: Some(8_200_000),
                    ..stream(StreamKind::Video, 0)
                },
                crate::api::StreamInfo {
                    display_title: Some("English - AC3 - 5.1".into()),
                    ..stream(StreamKind::Audio, 1)
                },
                crate::api::StreamInfo { language: Some("eng".into()), ..stream(StreamKind::Subtitle, 2) },
                crate::api::StreamInfo { display_title: Some("French".into()), ..stream(StreamKind::Subtitle, 3) },
            ],
        }
    }

    #[test]
    fn media_info_lines_cover_everything() {
        let lines = media_info_lines(&info(), true);
        let get = |k: &str| lines.iter().find(|(l, _)| l == k).map(|(_, v)| v.as_str());
        assert_eq!(get("Container"), Some("mkv"));
        assert_eq!(get("Video"), Some("hevc · 3840×2160 · 8.2 Mbps"));
        assert_eq!(get("Audio"), Some("English - AC3 - 5.1"));
        assert_eq!(get("Subtitles"), Some("eng, French"));
        assert_eq!(get("Bitrate"), Some("9.0 Mbps"));
        assert_eq!(get("Size"), Some("1.4 GB"));
        assert_eq!(get("Path"), Some("/media/films/ember.mkv"));
    }

    #[test]
    fn media_info_path_hidden_for_non_admins() {
        let lines = media_info_lines(&info(), false);
        assert!(lines.iter().all(|(l, v)| l != "Path" && !v.contains("/media/")));
        assert!(lines.iter().any(|(l, _)| l == "Size"));
    }

    #[test]
    fn size_and_bitrate_formats() {
        assert_eq!(format_size(820_000_000), "820 MB");
        assert_eq!(format_size(2_500_000_000), "2.5 GB");
        assert_eq!(format_bitrate(640_000), "640 kbps");
        assert_eq!(format_bitrate(8_200_000), "8.2 Mbps");
    }

    #[test]
    fn trailer_local_wins_then_remote_then_none() {
        let local = Item { id: "t1".into(), ..Default::default() };
        let remote =
            vec![MediaUrl { url: " ".into(), name: None }, MediaUrl { url: "https://youtu.be/x".into(), name: None }];
        assert_eq!(trailer_target(std::slice::from_ref(&local), &remote), Some(TrailerTarget::Local(Box::new(local))));
        assert_eq!(trailer_target(&[], &remote), Some(TrailerTarget::Remote("https://youtu.be/x".into())));
        assert_eq!(trailer_target(&[], &[]), None);
    }

    #[test]
    fn version_labels_use_names_then_numbers() {
        let src = MediaSource { id: "s1".into(), ..Default::default() };
        assert_eq!(version_label(0, Some(&src), &[info()]), "Ember Rising - 4K");
        assert_eq!(version_label(1, None, &[]), "Version 2");
    }
}
