//! Item pages (R-UI-6, R-UI-7): movie, series, season, episode. Placeholder look; every value from `Theme`.
use super::parts::{self, CardKind};
use super::player::{self, PlayOptions};
use crate::api::{
    image_width_bucket, ticks_to_secs, ApiError, ImageKind, Item, ItemKind, MediaStream, Result, StreamKind,
};
use crate::app::{Cx, Screen};
use crate::nav::{move_focus, Focus, Group};
use crate::platform::Key;

const G_BTN: usize = 0;
const G_LIST: usize = 1;
const G_RETRY: usize = 2;

#[derive(Clone, Copy)]
enum Action {
    Play { from_start: bool },
    PlayNext,
    ToggleWatched,
    OpenAudio,
    OpenSubtitles,
    AudioPick(i32),
    SubtitlePick(Option<i32>),
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
    focus: Focus,
    groups: Vec<Group>,
    started: bool,
}

pub fn screen(item: Item) -> Box<dyn Screen> {
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
        focus: Focus { group: G_BTN, index: 0 },
        groups: vec![],
        started: false,
    })
}

impl ItemScreen {
    fn current(&self) -> &Item {
        self.full.as_ref().unwrap_or(&self.item)
    }

    fn source(&self) -> Option<&crate::api::MediaSource> {
        self.current().media_sources.first()
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
        let played = self.current().user_data.as_ref().map(|u| u.played).unwrap_or(false);
        v.push((if played { "Mark unwatched" } else { "Mark watched" }.to_string(), Action::ToggleWatched));
        v.push((format!("Audio: {}", self.audio_label()), Action::OpenAudio));
        v.push((format!("Subtitles: {}", self.subtitle_label()), Action::OpenSubtitles));
        v
    }

    fn audio_button_index(&self) -> usize {
        self.movie_buttons().iter().position(|(_, a)| matches!(a, Action::OpenAudio)).unwrap_or(0)
    }

    fn subtitle_button_index(&self) -> usize {
        self.movie_buttons().iter().position(|(_, a)| matches!(a, Action::OpenSubtitles)).unwrap_or(0)
    }

    fn play_options(&self, from_start: bool) -> PlayOptions {
        PlayOptions { from_start, audio: self.audio.or(Some(0)), subtitle: self.subtitle.or(Some(None)) }
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
        match self.kind {
            ItemKind::Series => {
                let sid = self.item.id.clone();
                let client = cx.api();
                let sid2 = sid.clone();
                cx.spawn("seasons", move || client.seasons(&sid2));
                let client = cx.api();
                cx.spawn("nextup", move || client.next_up(20));
                let client = cx.api();
                cx.spawn("episodes", move || client.episodes(&sid, None));
            }
            ItemKind::Season => {
                let sid = self.item.series_id.clone().unwrap_or_default();
                let season = self.item.id.clone();
                let client = cx.api();
                cx.spawn("episodes", move || client.episodes(&sid, Some(&season)));
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

    fn do_action(&mut self, cx: &mut Cx, a: Action) {
        match a {
            Action::Play { from_start } => {
                let item = self.current().clone();
                let opts = self.play_options(from_start);
                cx.push(player::screen(item, opts));
            }
            Action::PlayNext => self.play_next_episode(cx),
            Action::ToggleWatched => self.toggle_watched(cx),
            Action::OpenAudio => {
                self.audio_open = true;
                self.subtitle_open = false;
            }
            Action::OpenSubtitles => {
                self.subtitle_open = true;
                self.audio_open = false;
            }
            Action::AudioPick(i) => {
                self.audio = Some(i);
                self.audio_open = false;
            }
            Action::SubtitlePick(v) => {
                self.subtitle = Some(v);
                self.subtitle_open = false;
            }
        }
    }

    fn enter_movie_episode(&mut self, cx: &mut Cx) -> bool {
        if self.focus.group == G_RETRY {
            self.spawn_loads(cx);
            self.error = None;
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

    fn draw_movie_episode(
        &mut self,
        ui: &mut egui::Ui,
        cx: &mut Cx,
        btn_xs: &mut Vec<f32>,
        list_xs: &mut Vec<f32>,
    ) -> Option<Action> {
        let t = cx.theme;
        let buttons = self.movie_buttons();
        let mut act = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = t.gap / 2.0;
            for (i, (label, action)) in buttons.iter().enumerate() {
                let focused = self.focus.group == G_BTN && self.focus.index == i;
                let r = parts::button(ui, t, label, focused, i == 0);
                btn_xs.push(r.rect.center().x);
                if r.hovered() {
                    self.focus = Focus { group: G_BTN, index: i };
                }
                if r.clicked() {
                    act = Some(*action);
                }
            }
        });
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
                if focused {
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

fn truncate_overview(ov: &str) -> String {
    let s = ov.trim();
    if s.chars().count() > 340 {
        let mut t: String = s.chars().take(340).collect();
        t.push('…');
        t
    } else {
        s.to_string()
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

    fn on_key(&mut self, cx: &mut Cx, key: Key) -> bool {
        match key {
            Key::Enter => match self.kind {
                ItemKind::Movie | ItemKind::Episode => self.enter_movie_episode(cx),
                ItemKind::Series => self.enter_series(cx),
                ItemKind::Season => self.enter_season(cx),
                _ => false,
            },
            Key::Back => {
                if self.audio_open || self.subtitle_open {
                    self.audio_open = false;
                    self.subtitle_open = false;
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
            _ => {}
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        if !self.started {
            self.started = true;
            self.spawn_loads(cx);
        }
        self.init_tracks();
        let t = cx.theme;

        {
            let item = self.current();
            let rect = parts::backdrop(ui, cx, item);
            let mut col = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink(t.margin)));
            col.set_max_width(t.field_w * 1.5);
            col.add_space(t.backdrop_h * 0.15);
            if let Some(line) = self.above_line(item) {
                col.label(egui::RichText::new(line).size(t.small_size).color(t.text));
                col.add_space(t.gap / 4.0);
            }
            parts::title(&mut col, t, &item.name);
            let meta = parts::meta_line(item);
            if !meta.is_empty() {
                col.add_space(t.gap / 4.0);
                col.label(egui::RichText::new(meta).size(t.small_size).color(t.muted));
            }
            if let Some(ov) = &item.overview {
                let ov = truncate_overview(ov);
                if !ov.is_empty() {
                    col.add_space(t.gap / 2.0);
                    col.add(egui::Label::new(egui::RichText::new(ov).size(t.body_size).color(t.muted)).wrap());
                }
            }
        }

        ui.add_space(t.gap);
        let mut act: Option<Action> = None;
        let mut push_item: Option<Item> = None;
        let mut btn_xs: Vec<f32> = vec![];
        let mut list_xs: Vec<f32> = vec![];
        let mut row_xs: Vec<f32> = vec![];

        match self.kind {
            ItemKind::Movie | ItemKind::Episode => {
                act = self.draw_movie_episode(ui, cx, &mut btn_xs, &mut list_xs);
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

        let mut groups = vec![Group::default(); 3];
        match self.kind {
            ItemKind::Movie | ItemKind::Episode => {
                let n = btn_xs.len();
                groups[G_BTN] = Group { len: n, cols: n, xs: btn_xs };
                if self.audio_open || self.subtitle_open {
                    groups[G_LIST] = Group { len: list_xs.len(), cols: 1, xs: list_xs };
                }
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
        if !retry_xs.is_empty() {
            groups[G_RETRY] = Group { len: 1, cols: 1, xs: retry_xs };
        }
        self.groups = groups;
        self.focus = move_focus(&self.groups, self.focus, Key::Enter);

        if let Some(a) = act {
            self.do_action(cx, a);
        }
        if let Some(it) = push_item {
            cx.push(super::item::screen(it));
        }
    }

    fn on_resume(&mut self, cx: &mut Cx) {
        self.refresh(cx);
    }
}
