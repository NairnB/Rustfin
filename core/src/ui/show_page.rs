//! F1 show page (SPEC R-F1-3): hero (backdrop, title, year · rating, overview, Play/Resume), a season
//! drop-down, and the episode list of the open season. Built from one `episodes(series_id, None)` request.
use super::parts::{self, CardKind};
use super::player::PlayOptions;
use crate::api::{image_width_bucket, ticks_to_secs, ApiError, Item, Result};
use crate::app::{Cx, Screen};
use crate::nav::{move_focus, Focus, Group};
use crate::platform::Key;
use crate::show::{self, PlayPick, ShowPlan};

const G_HERO: usize = 0;
const G_LIST: usize = 1;
const G_DROP: usize = 2;
const G_RETRY: usize = 3;

pub struct ShowPage {
    series: Item,
    requested_season: Option<i32>,
    episodes: Vec<Item>,
    // RR5 (P-TV-7): episodes the server lists that have not aired yet (shown only with the missing-episodes option).
    unaired: Vec<Item>,
    // RR5 (P-TV-2): the server's next-up episode, when it answered with one.
    next_up: Option<Item>,
    loading: bool,
    error: Option<String>,
    initialized: bool,
    open_season: i32,
    drop_open: bool,
    focus: Focus,
    groups: Vec<Group>,
    started: bool,
}

pub fn screen(series: Item, season: Option<i32>) -> Box<dyn Screen> {
    Box::new(ShowPage {
        series,
        requested_season: season,
        episodes: vec![],
        unaired: vec![],
        next_up: None,
        loading: true,
        error: None,
        initialized: false,
        open_season: 0,
        drop_open: false,
        focus: Focus { group: G_HERO, index: 0 },
        groups: vec![],
        started: false,
    })
}

/// A Season item opens the show page: fetch the series item first, then open it with that season selected.
pub struct SeasonOpen {
    season: Item,
    error: Option<String>,
    started: bool,
}

pub fn screen_for_season(season: Item) -> Box<dyn Screen> {
    Box::new(SeasonOpen { season, error: None, started: false })
}

impl Screen for SeasonOpen {
    fn name(&self) -> &'static str {
        "season"
    }

    fn on_key(&mut self, _cx: &mut Cx, key: Key) -> bool {
        if self.error.is_some() {
            return matches!(key, Key::Enter);
        }
        false
    }

    fn on_done(&mut self, cx: &mut Cx, tag: &'static str, payload: Box<dyn std::any::Any + Send>) {
        if tag != "series" {
            return;
        }
        let Ok(r) = payload.downcast::<Result<Item>>() else { return };
        match *r {
            Ok(series) => {
                let season = self.season.index_number;
                cx.pop();
                cx.push(super::show_page::screen(series, season));
            }
            Err(ApiError::Unauthorized) => cx.signed_out(),
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        if !self.started {
            self.started = true;
            let sid = self.season.series_id.clone().unwrap_or_default();
            let client = cx.api();
            cx.spawn("series", move || client.item(&sid));
        }
        let t = cx.theme;
        ui.add_space(t.margin);
        parts::title(ui, t, &self.season.name);
        if let Some(e) = self.error.clone() {
            ui.add_space(t.gap / 2.0);
            parts::error_line(ui, t, &e);
            let r = parts::button(ui, t, "Retry", true, false);
            if r.clicked() {
                self.error = None;
                let sid = self.season.series_id.clone().unwrap_or_default();
                let client = cx.api();
                cx.spawn("series", move || client.item(&sid));
            }
        } else {
            parts::muted(ui, t, "Loading…");
        }
    }
}

impl ShowPage {
    fn load(&mut self, cx: &mut Cx) {
        self.loading = true;
        let sid = self.series.id.clone();
        let client = cx.api();
        let missing = cx.settings.show_missing_episodes;
        cx.spawn("episodes", move || client.episodes_opts(&sid, missing));
        self.load_next_up(cx);
        // Card copies of a series carry no overview/rating/user data: fetch the full record in the background
        // (the page is drawn from the episode list meanwhile; the heart reads `user_data.is_favorite`).
        if self.series.user_data.is_none() || self.series.overview.is_none() {
            let sid = self.series.id.clone();
            let client = cx.api();
            cx.spawn("series-full", move || client.item(&sid));
        }
    }

    fn refresh(&mut self, cx: &mut Cx) {
        let sid = self.series.id.clone();
        let client = cx.api();
        let missing = cx.settings.show_missing_episodes;
        cx.spawn("episodes", move || client.episodes_opts(&sid, missing));
        self.load_next_up(cx);
    }

    /// P-TV-2: ask the server which episode is next up for this series.
    fn load_next_up(&self, cx: &Cx) {
        let sid = self.series.id.clone();
        let client = cx.api();
        cx.spawn("next-up", move || client.series_next_up(&sid));
    }

    /// The show plan from the aired episodes; the server's next-up episode (when it answered) is the Play button,
    /// and seasons that so far hold only unaired episodes still get a drop-down entry.
    fn plan(&self) -> ShowPlan {
        let mut plan = show::plan(&self.episodes);
        for u in &self.unaired {
            if let Some(s) = u.parent_index_number {
                if !plan.seasons.contains(&s) {
                    plan.seasons.push(s);
                }
            }
        }
        plan.seasons.sort_by_key(|&s| if s == 0 { i32::MAX } else { s });
        if let Some(pick) = self.next_up.as_ref().and_then(server_pick) {
            plan.play = Some(pick);
        }
        plan
    }

    fn season_label(&self, s: i32) -> String {
        if s == 0 {
            "Specials".to_string()
        } else {
            format!("Season {s}")
        }
    }

    /// Indices into `self.episodes` for the open season, in episode order.
    /// The rows of the open season in episode order: aired episodes and (with the option on) unaired ones.
    fn season_rows(&self) -> Vec<Item> {
        let mut v: Vec<Item> = self
            .episodes
            .iter()
            .chain(self.unaired.iter())
            .filter(|e| e.parent_index_number == Some(self.open_season) && e.index_number.is_some())
            .cloned()
            .collect();
        v.sort_by_key(|e| e.index_number.unwrap());
        v
    }

    /// The hero buttons in order: Play, season drop-down, Shuffle, Mark season, Mark series, Missing, heart.
    fn hero_buttons(plan: &ShowPlan) -> Vec<Hero> {
        let mut v = vec![];
        if plan.play.is_some() {
            v.push(Hero::Play);
        }
        if !plan.seasons.is_empty() {
            v.push(Hero::Drop);
            v.push(Hero::Shuffle);
            v.push(Hero::MarkSeason);
            v.push(Hero::MarkSeries);
        }
        v.push(Hero::Missing);
        v.push(Hero::Heart);
        v.push(Hero::More);
        v
    }

    fn hero_idx(plan: &ShowPlan, which: Hero) -> Option<usize> {
        Self::hero_buttons(plan).iter().position(|&h| h == which)
    }

    fn hero_drop_idx(plan: &ShowPlan) -> Option<usize> {
        Self::hero_idx(plan, Hero::Drop)
    }

    /// Real episodes (with an episode number) of the open season / the whole series.
    fn all_played(&self, season_only: bool) -> bool {
        let mut it = self
            .episodes
            .iter()
            .filter(|e| e.index_number.is_some() && (!season_only || e.parent_index_number == Some(self.open_season)))
            .peekable();
        it.peek().is_some() && it.all(|e| e.user_data.as_ref().map(|u| u.played).unwrap_or(false))
    }

    /// P-TV-5: mark the open season / the series played or unplayed; the server cascades to the episodes.
    fn mark(&mut self, cx: &mut Cx, season_only: bool) {
        let played = !self.all_played(season_only);
        let id = if season_only {
            match self.episodes.iter().find(|e| e.parent_index_number == Some(self.open_season)) {
                Some(e) => e.season_id.clone().unwrap_or_default(),
                None => return,
            }
        } else {
            self.series.id.clone()
        };
        if id.is_empty() {
            return;
        }
        let client = cx.api();
        cx.spawn("played", move || client.set_played(&id, played));
    }

    /// P-TV-10: a random unplayed episode first (any episode when all are played), then the rest in random order.
    fn shuffle(&mut self, cx: &mut Cx) {
        let q = shuffle_queue(&self.episodes, seed());
        if !q.is_empty() {
            cx.push(super::player::screen_queue(q, PlayOptions { from_start: true, ..Default::default() }));
        }
    }

    /// P-TV-7: flip the "show missing episodes" setting and reload the list.
    fn toggle_missing(&mut self, cx: &mut Cx) {
        cx.settings.show_missing_episodes = !cx.settings.show_missing_episodes;
        cx.save_settings();
        self.refresh(cx);
    }

    fn press(&mut self, cx: &mut Cx, plan: &ShowPlan, which: Hero) {
        match which {
            Hero::Play => {
                if let Some(pick) = plan.play.clone() {
                    self.play_pick(cx, &pick);
                }
            }
            Hero::Drop => {
                self.drop_open = true;
                let idx = plan.seasons.iter().position(|&s| s == self.open_season).unwrap_or(0);
                self.focus = Focus { group: G_DROP, index: idx };
            }
            Hero::Shuffle => self.shuffle(cx),
            Hero::MarkSeason => self.mark(cx, true),
            Hero::MarkSeries => self.mark(cx, false),
            Hero::Missing => self.toggle_missing(cx),
            Hero::Heart => self.toggle_favorite(cx),
            Hero::More => cx.push(super::admin_actions::more_screen(self.series.clone(), cx.admin)),
        }
    }

    fn toggle_favorite(&mut self, cx: &mut Cx) {
        let fav = self.series.user_data.as_ref().map(|u| u.is_favorite).unwrap_or(false);
        let id = self.series.id.clone();
        let client = cx.api();
        cx.spawn("favorite", move || client.set_favorite(&id, !fav));
    }

    fn play_pick(&mut self, cx: &mut Cx, pick: &crate::show::PlayPick) {
        let found = self.episodes.iter().chain(self.next_up.iter()).find(|e| e.id == pick.episode_id).cloned();
        if let Some(ep) = found {
            cx.push(super::player::screen(ep, PlayOptions { from_start: !pick.resume, ..Default::default() }));
        }
    }

    fn play_episode(&mut self, cx: &mut Cx, ep: &Item) {
        let resume = ep.user_data.as_ref().map(|u| u.playback_position_ticks > 0 && !u.played).unwrap_or(false);
        cx.push(super::player::screen(ep.clone(), PlayOptions { from_start: !resume, ..Default::default() }));
    }

    fn draw_hero(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        let t = cx.theme;
        // G12 / P-USER-2: backdrops can be switched off in Settings.
        let rect = if cx.settings.backdrops {
            parts::backdrop_sized(ui, cx, &self.series, t.hero_h)
        } else {
            let (alloc, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), t.hero_h), egui::Sense::hover());
            ui.painter().rect_filled(alloc, 0.0, t.surface);
            alloc
        };
        let mut col = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink(t.margin)));
        col.set_max_width(t.field_w * 1.5);
        col.add_space(t.hero_h * 0.15);
        parts::title(&mut col, t, &self.series.name);
        let meta = parts::meta_line(&self.series);
        if !meta.is_empty() {
            col.add_space(t.gap / 4.0);
            col.label(egui::RichText::new(meta).size(t.small_size).color(t.muted));
        }
        if let Some(ov) = &self.series.overview {
            let ov = truncate(ov);
            if !ov.is_empty() {
                col.add_space(t.gap / 2.0);
                col.add(egui::Label::new(egui::RichText::new(ov).size(t.body_size).color(t.muted)).wrap());
            }
        }
        // P-TV-2: the server's next-up episode, as a card on the right of the hero.
        if let Some(next) = self.next_up.clone() {
            let w = t.thumb_w;
            let area = egui::Rect::from_min_max(
                egui::pos2(rect.right() - t.margin - w, rect.top() + t.margin),
                egui::pos2(rect.right() - t.margin, rect.bottom()),
            );
            let mut side = ui.new_child(egui::UiBuilder::new().max_rect(area));
            side.label(egui::RichText::new("Next up").size(t.small_size).color(t.text));
            side.add_space(t.gap / 4.0);
            parts::thumb_card(&mut side, cx, &next, false);
        }
    }

    fn draw_episode_list(
        &mut self,
        ui: &mut egui::Ui,
        cx: &mut Cx,
        list_xs: &mut Vec<f32>,
        clicked: &mut Option<Item>,
    ) {
        let t = cx.theme;
        let rows = self.season_rows();
        let today = today();
        if rows.is_empty() {
            parts::muted(ui, t, "No episodes in this season.");
            return;
        }
        let moving = ui.input(|i| i.pointer.is_moving());
        let thumb_w = t.ep_thumb_w;
        let thumb_h = t.ep_thumb_h;
        let row_h = thumb_h + t.gap / 2.0;
        let ppp = ui.ctx().pixels_per_point();
        egui::ScrollArea::vertical().id_salt("show-episodes").auto_shrink([false, false]).show(ui, |ui| {
            for (i, ep) in rows.iter().enumerate() {
                let unaired = is_unaired(ep, today);
                let focused = self.focus.group == G_LIST && self.focus.index == i;
                let (rect, resp) =
                    ui.allocate_exact_size(egui::vec2(ui.available_width(), row_h), egui::Sense::click());
                if ui.is_rect_visible(rect) {
                    let thumb_rect = egui::Rect::from_min_size(rect.min, egui::vec2(thumb_w, thumb_h));
                    let w = image_width_bucket(thumb_w, ppp);
                    let url = cx.client.as_ref().and_then(|c| c.image_url(ep, crate::api::ImageKind::Primary, w));
                    match url.as_deref().and_then(|u| cx.images.get(u)) {
                        Some(tex) => {
                            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                            ui.painter().image(tex, thumb_rect, uv, egui::Color32::WHITE);
                        }
                        None => {
                            ui.painter().rect_filled(thumb_rect, t.radius, t.skeleton);
                        }
                    }
                    if unaired {
                        ui.painter().rect_filled(thumb_rect, t.radius, t.scrim);
                    }
                    if let Some(f) = parts::resume_fraction(ep) {
                        parts::progress_bar(ui, t, thumb_rect, f);
                    }
                    if focused {
                        parts::focus_ring(ui, t, thumb_rect);
                    }
                    let x = thumb_rect.right() + t.gap / 2.0;
                    let w_text = rect.right() - x - t.gap;
                    let ty = rect.top() + 4.0;
                    let title = format!("E{} · {}", ep.index_number.unwrap_or(0), ep.name);
                    parts::label_line(ui, t, egui::pos2(x, ty), w_text, &title, t.body_size, t.text);
                    let ry = ty + t.body_size + 4.0;
                    let runtime = match (unaired, airs_label(ep)) {
                        (true, Some(l)) => l,
                        _ => ep.run_time_ticks.map(|rt| parts::runtime(ticks_to_secs(rt))).unwrap_or_default(),
                    };
                    parts::label_line(ui, t, egui::pos2(x, ry), w_text, &runtime, t.small_size, t.muted);
                    let played = ep.user_data.as_ref().map(|u| u.played).unwrap_or(false);
                    if played {
                        let rw =
                            ui.fonts(|f| f.layout_no_wrap(runtime.clone(), t.font(t.small_size), t.muted).size().x);
                        let center = egui::pos2(x + rw + t.gap / 3.0 + t.small_size * 0.5, ry + t.small_size * 0.5);
                        parts::tick(ui, t, center, t.small_size);
                    }
                    if let Some(ov) = &ep.overview {
                        let ov = ov.trim();
                        if !ov.is_empty() {
                            let oy = ry + t.small_size + 4.0;
                            parts::two_line(ui, t, egui::pos2(x, oy), w_text, ov, t.small_size, t.muted);
                        }
                    }
                }
                list_xs.push(rect.center().x);
                if resp.hovered() && moving {
                    self.focus = Focus { group: G_LIST, index: i };
                }
                if focused {
                    resp.scroll_to_me(None);
                }
                if resp.clicked() && !unaired {
                    *clicked = Some(ep.clone());
                }
            }
        });
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Hero {
    Play,
    Drop,
    Shuffle,
    MarkSeason,
    MarkSeries,
    Missing,
    Heart,
    More,
}

/// P-TV-2: the Play button for the server's next-up episode: "Play S3 E1", or "Resume S3 E1 · 12 min left".
pub fn server_pick(ep: &Item) -> Option<PlayPick> {
    let (s, e) = (ep.parent_index_number?, ep.index_number?);
    let pos = ep.user_data.as_ref().map(|u| u.playback_position_ticks).unwrap_or(0);
    let played = ep.user_data.as_ref().map(|u| u.played).unwrap_or(false);
    if pos > 0 && !played {
        let total = ep.run_time_ticks.map(ticks_to_secs).unwrap_or(0.0);
        let m = (((total - ticks_to_secs(pos)).max(0.0) / 60.0).ceil() as i64).max(1);
        Some(PlayPick { episode_id: ep.id.clone(), label: format!("Resume S{s} E{e} · {m} min left"), resume: true })
    } else {
        Some(PlayPick { episode_id: ep.id.clone(), label: format!("Play S{s} E{e}"), resume: false })
    }
}

/// The `(year, month, day)` at the front of an episode's premiere date.
fn air_date(ep: &Item) -> Option<(i32, u32, u32)> {
    let d = ep.premiere_date.as_deref()?.get(..10)?;
    let mut it = d.split('-');
    Some((it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?))
}

/// Today's UTC date as `(year, month, day)`.
pub fn today() -> (i32, u32, u32) {
    let days = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| (d.as_secs() / 86_400) as i64)
        .unwrap_or(0);
    civil_from_days(days)
}

/// Days since 1970-01-01 → (year, month, day) (proleptic Gregorian).
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = (yoe + era * 400) as i32 + (m <= 2) as i32;
    (y, m, d)
}

/// P-TV-7: an episode whose premiere date is after `today`.
pub fn is_unaired(ep: &Item, today: (i32, u32, u32)) -> bool {
    air_date(ep).is_some_and(|d| d > today)
}

/// P-TV-7: "Airs 14 Oct".
pub fn airs_label(ep: &Item) -> Option<String> {
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let (_, m, d) = air_date(ep)?;
    Some(format!("Airs {d} {}", MONTHS.get(m.checked_sub(1)? as usize)?))
}

fn seed() -> u64 {
    let mut b = [0u8; 8];
    let _ = getrandom::fill(&mut b);
    u64::from_le_bytes(b)
        ^ std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0)
}

/// P-TV-10: the unplayed episodes (all of them when none is unplayed) in a random order from `seed`
/// (xorshift + Fisher–Yates), so the first one is a random unwatched episode and the rest follow at random.
pub fn shuffle_queue(episodes: &[Item], seed: u64) -> Vec<Item> {
    let real: Vec<&Item> = episodes.iter().filter(|e| e.index_number.is_some()).collect();
    let unplayed: Vec<&Item> =
        real.iter().copied().filter(|e| !e.user_data.as_ref().map(|u| u.played).unwrap_or(false)).collect();
    let mut v: Vec<Item> = if unplayed.is_empty() { real } else { unplayed }.into_iter().cloned().collect();
    let mut x = seed | 1;
    for i in (1..v.len()).rev() {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        v.swap(i, (x % (i as u64 + 1)) as usize);
    }
    v
}

fn truncate(ov: &str) -> String {
    let s = ov.trim();
    if s.chars().count() > 340 {
        let mut t: String = s.chars().take(340).collect();
        t.push('…');
        t
    } else {
        s.to_string()
    }
}

impl Screen for ShowPage {
    fn name(&self) -> &'static str {
        "show"
    }

    fn on_key(&mut self, cx: &mut Cx, key: Key) -> bool {
        if self.error.is_some() {
            return match key {
                Key::Enter => {
                    self.error = None;
                    self.load(cx);
                    true
                }
                _ => false,
            };
        }
        if self.loading {
            return false;
        }
        match key {
            Key::Enter => {
                let plan = self.plan();
                if self.drop_open {
                    if let Some(&s) = plan.seasons.get(self.focus.index) {
                        self.open_season = s;
                        self.drop_open = false;
                        self.focus = Focus { group: G_HERO, index: Self::hero_drop_idx(&plan).unwrap_or(0) };
                    }
                    return true;
                }
                if self.focus.group == G_HERO {
                    if let Some(&h) = Self::hero_buttons(&plan).get(self.focus.index) {
                        self.press(cx, &plan, h);
                    }
                    return true;
                }
                if self.focus.group == G_LIST {
                    if let Some(ep) = self.season_rows().get(self.focus.index).cloned() {
                        if !is_unaired(&ep, today()) {
                            self.play_episode(cx, &ep);
                        }
                    }
                    return true;
                }
                false
            }
            Key::Back => {
                if self.drop_open {
                    self.drop_open = false;
                    let plan = self.plan();
                    self.focus = Focus { group: G_HERO, index: Self::hero_drop_idx(&plan).unwrap_or(0) };
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
        if tag == "played" {
            let Ok(r) = payload.downcast::<Result<()>>() else { return };
            match *r {
                Ok(()) => {
                    self.refresh(cx);
                    let sid = self.series.id.clone();
                    let client = cx.api();
                    cx.spawn("series-full", move || client.item(&sid));
                }
                Err(ApiError::Unauthorized) => cx.signed_out(),
                Err(e) => self.error = Some(e.to_string()),
            }
            return;
        }
        if tag == "favorite" {
            let Ok(r) = payload.downcast::<Result<()>>() else { return };
            match *r {
                Ok(()) => {
                    let sid = self.series.id.clone();
                    let client = cx.api();
                    cx.spawn("series-full", move || client.item(&sid));
                }
                Err(ApiError::Unauthorized) => cx.signed_out(),
                Err(e) => self.error = Some(e.to_string()),
            }
            return;
        }
        if tag == "series-full" {
            if let Ok(r) = payload.downcast::<Result<Item>>() {
                if let Ok(full) = *r {
                    self.series = full;
                }
            }
            return;
        }
        if tag == "next-up" {
            if let Ok(r) = payload.downcast::<Result<Option<Item>>>() {
                match *r {
                    Ok(v) => self.next_up = v,
                    Err(ApiError::Unauthorized) => cx.signed_out(),
                    Err(_) => self.next_up = None,
                }
            }
            return;
        }
        if tag != "episodes" {
            return;
        }
        self.loading = false;
        let Ok(r) = payload.downcast::<Result<Vec<Item>>>() else { return };
        match *r {
            Ok(v) => {
                // P-TV-7: with the missing-episodes option off, nothing that has not aired shows either.
                let today = today();
                let (unaired, aired): (Vec<Item>, Vec<Item>) = v.into_iter().partition(|e| is_unaired(e, today));
                self.episodes = aired;
                self.unaired = if cx.settings.show_missing_episodes { unaired } else { vec![] };
                self.error = None;
                if !self.initialized {
                    self.initialized = true;
                    let plan = self.plan();
                    self.open_season = self.requested_season.unwrap_or(plan.open_season);
                }
            }
            Err(ApiError::Unauthorized) => cx.signed_out(),
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn on_resume(&mut self, cx: &mut Cx) {
        self.refresh(cx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        if !self.started {
            self.started = true;
            self.load(cx);
        }
        let t = cx.theme;

        if self.loading {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), t.hero_h), egui::Sense::hover());
            ui.painter().rect_filled(rect, 0.0, t.skeleton);
            ui.add_space(t.gap / 2.0);
            for _ in 0..4 {
                parts::skeleton_card(ui, t, CardKind::Thumb);
                ui.add_space(t.gap / 2.0);
            }
            self.groups = vec![];
            return;
        }

        if let Some(e) = self.error.clone() {
            ui.add_space(t.gap);
            parts::error_line(ui, t, &e);
            let focused = self.focus.group == G_RETRY;
            let r = parts::button(ui, t, "Retry", focused, false);
            let xs = vec![r.rect.center().x];
            if r.hovered() {
                self.focus = Focus { group: G_RETRY, index: 0 };
            }
            if r.clicked() {
                self.error = None;
                self.load(cx);
            }
            self.groups = vec![Group { len: 1, cols: 1, xs }];
            return;
        }

        let plan = self.plan();
        let moving = ui.input(|i| i.pointer.is_moving());
        self.draw_hero(ui, cx);

        let mut hero_xs = vec![];
        let mut drop_xs = vec![];
        let mut list_xs = vec![];
        let mut pressed: Option<Hero> = None;
        let mut drop_pick: Option<i32> = None;
        let mut ep_click: Option<Item> = None;

        ui.add_space(t.gap / 2.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = t.gap / 2.0;
            for (idx, h) in Self::hero_buttons(&plan).into_iter().enumerate() {
                let focused = self.focus.group == G_HERO && self.focus.index == idx;
                let r = match h {
                    Hero::Play => parts::button(ui, t, &plan.play.as_ref().unwrap().label, focused, true),
                    Hero::Drop => {
                        let label = format!("{} (change)", self.season_label(self.open_season));
                        parts::button_width(ui, t, &label, focused, false, t.dropdown_w)
                    }
                    Hero::Shuffle => parts::button(ui, t, "Shuffle", focused, false),
                    Hero::MarkSeason => {
                        let label = if self.all_played(true) { "Mark season unwatched" } else { "Mark season watched" };
                        parts::button(ui, t, label, focused, false)
                    }
                    Hero::MarkSeries => {
                        let label =
                            if self.all_played(false) { "Mark series unwatched" } else { "Mark series watched" };
                        parts::button(ui, t, label, focused, false)
                    }
                    Hero::Missing => {
                        let label = if cx.settings.show_missing_episodes {
                            "Missing episodes: shown"
                        } else {
                            "Missing episodes: hidden"
                        };
                        parts::button(ui, t, label, focused, false)
                    }
                    Hero::Heart => {
                        let fav = self.series.user_data.as_ref().map(|u| u.is_favorite).unwrap_or(false);
                        parts::heart_button(ui, t, fav, focused)
                    }
                    Hero::More => parts::button(ui, t, "More…", focused, false),
                };
                hero_xs.push(r.rect.center().x);
                if r.hovered() && moving {
                    self.focus = Focus { group: G_HERO, index: idx };
                }
                if r.clicked() {
                    pressed = Some(h);
                }
            }
        });

        if self.drop_open {
            ui.add_space(t.gap / 2.0);
            for (i, &s) in plan.seasons.iter().enumerate() {
                let focused = self.focus.group == G_DROP && self.focus.index == i;
                let r = parts::button_width(ui, t, &self.season_label(s), focused, false, t.dropdown_w);
                drop_xs.push(r.rect.center().x);
                if r.hovered() && moving {
                    self.focus = Focus { group: G_DROP, index: i };
                }
                if r.clicked() {
                    drop_pick = Some(s);
                }
            }
        } else {
            self.draw_episode_list(ui, cx, &mut list_xs, &mut ep_click);
        }

        let mut groups = vec![Group::default(); 4];
        groups[G_HERO] = Group { len: hero_xs.len(), cols: hero_xs.len(), xs: hero_xs };
        if self.drop_open {
            groups[G_DROP] = Group { len: drop_xs.len(), cols: 1, xs: drop_xs };
        } else {
            groups[G_LIST] = Group { len: list_xs.len(), cols: 1, xs: list_xs };
        }
        self.groups = groups;
        self.focus = move_focus(&self.groups, self.focus, Key::Enter);

        if let Some(h) = pressed {
            self.press(cx, &plan, h);
        }
        if let Some(s) = drop_pick {
            self.open_season = s;
            self.drop_open = false;
            self.focus = Focus { group: G_HERO, index: Self::hero_drop_idx(&plan).unwrap_or(0) };
        }
        if let Some(ep) = ep_click {
            self.play_episode(cx, &ep);
        }
    }
}
