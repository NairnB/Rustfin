//! F1 show page (SPEC R-F1-3): hero (backdrop, title, year · rating, overview, Play/Resume), a season
//! drop-down, and the episodes of the open season. Built from one `episodes(series_id, None)` request.
//! D2: drawn in the Apple TV look (`at_details`): full-bleed hero, action row, a row of 16:9 episode cards,
//! Up Next, Cast & Crew and Information below.
use super::at_details::{self as at, ActionBtn, EpisodeCard, HeroArt, RoundIcon};
use super::parts::{self, CardKind};
use super::player::PlayOptions;
use super::widgets;
use crate::api::{ticks_to_secs, ApiError, ImageKind, Item, Result};
use crate::app::{Cx, Screen};
use crate::nav::{move_focus, Focus, Group};
use crate::platform::Key;
use crate::show::{self, PlayPick, ShowPlan};
use crate::theme::Theme;

const G_HERO: usize = 0;
const G_LIST: usize = 1;
const G_DROP: usize = 2;
const G_RETRY: usize = 3;
// D2: the sections below the episode row
const G_NEXT: usize = 4;
const G_CAST: usize = 5;
const G_INFO: usize = 6;
const N_GROUPS: usize = 7;

const OVERVIEW_LINES: usize = 4;

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

    /// D2: the hero buttons as the Apple TV action row (same order as `hero_buttons`, so key paths stay).
    fn action_btns(&self, cx: &Cx, plan: &ShowPlan) -> Vec<ActionBtn> {
        let round = |icon, on, caption: &str| ActionBtn::Round { icon, on, caption: caption.to_string() };
        Self::hero_buttons(plan)
            .into_iter()
            .map(|h| match h {
                Hero::Play => ActionBtn::Play(plan.play.as_ref().map(|p| p.label.clone()).unwrap_or_default()),
                Hero::Drop => ActionBtn::Menu(self.season_label(self.open_season)),
                Hero::Shuffle => round(RoundIcon::Shuffle, false, "Shuffle"),
                Hero::MarkSeason => {
                    let on = self.all_played(true);
                    round(RoundIcon::Watched, on, if on { "Mark season unwatched" } else { "Mark season watched" })
                }
                Hero::MarkSeries => {
                    let on = self.all_played(false);
                    round(RoundIcon::WatchedAll, on, if on { "Mark series unwatched" } else { "Mark series watched" })
                }
                Hero::Missing => ActionBtn::Pill(
                    if cx.settings.show_missing_episodes {
                        "Missing episodes: shown"
                    } else {
                        "Missing episodes: hidden"
                    }
                    .to_string(),
                ),
                Hero::Heart => {
                    let fav = self.series.user_data.as_ref().map(|u| u.is_favorite).unwrap_or(false);
                    round(RoundIcon::Favourite, fav, "Favourite")
                }
                Hero::More => round(RoundIcon::More, false, "More"),
            })
            .collect()
    }

    /// D2: full-bleed hero (backdrop unless switched off, P-USER-2), title logo or name, year · rating · seasons ·
    /// genres, overview, then the action row. Returns the action row's responses.
    fn draw_hero(&mut self, ui: &mut egui::Ui, cx: &mut Cx, btns: &[ActionBtn]) -> Vec<egui::Response> {
        let t = cx.theme;
        let backdrop = if cx.settings.backdrops {
            at::tex(cx, ui, &self.series, ImageKind::Backdrop, ui.ctx().screen_rect().width())
        } else {
            None
        };
        let art = HeroArt { backdrop, logo: at::logo(cx, ui, &self.series) };
        let rect = at::hero(ui, t, art, t.backdrop_h);
        let hero_focus = (self.focus.group == G_HERO).then_some(0);
        if at::focus_moved(ui, egui::Id::new("show-hero"), hero_focus) {
            ui.scroll_to_rect(rect.with_max_y(rect.top() + t.gap), None);
        }
        let mut col = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect.shrink2(egui::vec2(0.0, t.gap)))
                .layout(egui::Layout::bottom_up(egui::Align::Min)),
        );
        col.spacing_mut().item_spacing.y = 0.0;
        col.add_space(t.gap);
        let rs = at::action_row(&mut col, t, btns, (self.focus.group == G_HERO).then_some(self.focus.index));
        col.add_space(t.gap);
        if let Some(ov) = self.series.overview.as_deref().map(str::trim).filter(|o| !o.is_empty()) {
            at::overview(&mut col, t, ov, false, OVERVIEW_LINES);
            col.add_space(t.gap / 2.0);
        }
        let years = crate::story::year_line(&self.series, cx.settings.show_release_year, cx.settings.show_story_year);
        let mut meta = at::meta_parts(&Item { run_time_ticks: None, ..self.series.clone() }, &years);
        let n = self.plan().seasons.iter().filter(|&&s| s > 0).count();
        if n > 0 {
            let at_ix = meta.len().min(if self.series.official_rating.is_some() { 2 } else { 1 });
            meta.insert(at_ix, format!("{n} Season{}", if n == 1 { "" } else { "s" }));
        }
        at::meta_row(&mut col, t, &meta, (self.series.community_rating, self.series.critic_rating), &[]);
        col.add_space(t.gap / 2.0);
        at::hero_title(&mut col, t, &self.series.name, art.logo);
        rs
    }

    /// D2: the open season as a row of 16:9 episode cards (unaired ones dimmed, with their air date).
    fn draw_episode_row(
        &mut self,
        ui: &mut egui::Ui,
        cx: &mut Cx,
        list_xs: &mut Vec<f32>,
        clicked: &mut Option<Item>,
        moving: bool,
    ) {
        let t = cx.theme;
        at::section_title(ui, t, &self.season_label(self.open_season));
        let rows = self.season_rows();
        if rows.is_empty() {
            parts::muted(ui, t, "No episodes in this season.");
            return;
        }
        let today = today();
        let focus = (self.focus.group == G_LIST).then_some(self.focus.index);
        let rs = at::row(ui, t, "show-episodes", rows.len(), at::episode_card_h(t), focus, |ui, i| {
            let ep = &rows[i];
            let unaired = is_unaired(ep, today);
            let detail = match (unaired, airs_label(ep)) {
                (true, Some(l)) => l,
                _ => ep.run_time_ticks.map(|rt| at::runtime_long(ticks_to_secs(rt))).unwrap_or_default(),
            };
            let card = EpisodeCard {
                art: at::tex(cx, ui, ep, ImageKind::Primary, t.ep_thumb_w),
                number: match (self.open_season, ep.index_number) {
                    (0, _) | (_, None) => "Special".to_string(),
                    (_, Some(n)) => format!("Episode {n}"),
                },
                title: ep.name.clone(),
                overview: ep.overview.as_deref().map(str::trim).unwrap_or_default().to_string(),
                detail,
                progress: parts::resume_fraction(ep),
                played: ep.user_data.as_ref().map(|u| u.played).unwrap_or(false),
                unaired,
            };
            at::episode_card(ui, t, &card, focus == Some(i))
        });
        for (i, r) in rs.iter().enumerate() {
            list_xs.push(r.rect.center().x);
            if r.hovered() && moving {
                self.focus = Focus { group: G_LIST, index: i };
            }
            if r.clicked() && !is_unaired(&rows[i], today) {
                *clicked = Some(rows[i].clone());
            }
        }
    }

    /// D2: the season drop-down as a translucent menu card under the hero (rows are pills; focused = white plate).
    fn draw_season_menu(
        &mut self,
        ui: &mut egui::Ui,
        t: &Theme,
        plan: &ShowPlan,
        xs: &mut Vec<f32>,
        pick: &mut Option<i32>,
        moving: bool,
    ) {
        ui.add_space(t.gap);
        let n = plan.seasons.len() as f32;
        let row_h = t.button_h + t.gap / 4.0;
        let size = egui::vec2(t.dropdown_w + 2.0 * t.menu_pad, n * row_h + 2.0 * t.menu_pad);
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
        ui.painter().add(t.shadow(1.0).as_shape(rect, t.menu_radius));
        ui.painter().rect_filled(rect, t.menu_radius, t.menu_fill);
        let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink(t.menu_pad)));
        inner.spacing_mut().item_spacing.y = t.gap / 4.0;
        for (i, &s) in plan.seasons.iter().enumerate() {
            let focused = self.focus.group == G_DROP && self.focus.index == i;
            let r = widgets::pill_button_width(&mut inner, t, &self.season_label(s), focused, false, t.dropdown_w);
            xs.push(r.rect.center().x);
            if r.hovered() && moving {
                self.focus = Focus { group: G_DROP, index: i };
            }
            if r.clicked() {
                *pick = Some(s);
            }
        }
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
    let days = crate::time::SystemTime::now()
        .duration_since(crate::time::UNIX_EPOCH)
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
        ^ crate::time::SystemTime::now()
            .duration_since(crate::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0)
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

impl Screen for ShowPage {
    fn name(&self) -> &'static str {
        "show"
    }

    /// NAV: "Drift Rebels › Season 2" in the toolbar.
    fn crumbs(&self) -> Vec<String> {
        if self.episodes.is_empty() {
            vec![self.series.name.clone()]
        } else {
            vec![self.series.name.clone(), self.season_label(self.open_season)]
        }
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
                if self.focus.group == G_NEXT {
                    if let Some(ep) = self.next_up.clone() {
                        self.play_episode(cx, &ep);
                    }
                    return true;
                }
                if self.focus.group == G_CAST {
                    if let Some(p) = self.series.people.get(self.focus.index).cloned() {
                        cx.push(super::person::screen(p.id, p.name));
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
            at::hero(ui, t, HeroArt::default(), t.backdrop_h);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = t.gap;
                for _ in 0..4 {
                    parts::skeleton_card(ui, t, CardKind::Thumb);
                }
            });
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
        let btns = self.action_btns(cx, &plan);
        let mut hero_xs = vec![];
        let mut drop_xs = vec![];
        let mut list_xs = vec![];
        let mut next_xs = vec![];
        let mut cast_xs = vec![];
        let mut info_xs = vec![];
        let mut pressed: Option<Hero> = None;
        let mut drop_pick: Option<i32> = None;
        let mut ep_click: Option<Item> = None;
        let mut person: Option<crate::api::PersonRef> = None;

        egui::ScrollArea::vertical().id_salt("show-page").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let rs = self.draw_hero(ui, cx, &btns);
            for (idx, (r, h)) in rs.iter().zip(Self::hero_buttons(&plan)).enumerate() {
                hero_xs.push(r.rect.center().x);
                if r.hovered() && moving {
                    self.focus = Focus { group: G_HERO, index: idx };
                }
                if r.clicked() {
                    pressed = Some(h);
                }
            }
            if self.drop_open {
                self.draw_season_menu(ui, t, &plan, &mut drop_xs, &mut drop_pick, moving);
            } else {
                self.draw_episode_row(ui, cx, &mut list_xs, &mut ep_click, moving);
            }
            // P-TV-2: the server's next-up episode as a wide card (Enter / click plays it).
            if let Some(next) = self.next_up.clone() {
                at::section_title(ui, t, "Up Next");
                let focus = (self.focus.group == G_NEXT).then_some(0);
                let h = t.wide_h + t.card_title_gap + t.body_size * 1.3;
                let name = self.series.name.clone();
                let rs = at::row(ui, t, "show-next", 1, h, focus, |ui, _| {
                    let sub = match (next.parent_index_number, next.index_number) {
                        (Some(s), Some(e)) => format!("S{s}, E{e} · {}", next.name),
                        _ => next.name.clone(),
                    };
                    let tx = at::tex(cx, ui, &next, ImageKind::Primary, t.wide_w);
                    at::wide_card(ui, t, tx, &name, &sub, parts::resume_fraction(&next), focus.is_some())
                });
                for r in &rs {
                    next_xs.push(r.rect.center().x);
                    if r.hovered() && moving {
                        self.focus = Focus { group: G_NEXT, index: 0 };
                    }
                    if r.clicked() {
                        ep_click = Some(next.clone());
                    }
                }
            }
            let people = self.series.people.clone();
            if !people.is_empty() {
                at::section_title(ui, t, "Cast & Crew");
                let focus = (self.focus.group == G_CAST).then_some(self.focus.index);
                let rs = at::row(ui, t, "show-cast", people.len(), at::person_h(t), focus, |ui, i| {
                    let tx = at::person_tex(cx, ui, &people[i]);
                    at::person_circle(ui, t, tx, &people[i].name, &at::role_of(&people[i]), focus == Some(i))
                });
                for (i, r) in rs.iter().enumerate() {
                    cast_xs.push(r.rect.center().x);
                    if r.hovered() && moving {
                        self.focus = Focus { group: G_CAST, index: i };
                    }
                    if r.clicked() {
                        person = Some(people[i].clone());
                    }
                }
            }
            let cols = at::info_columns(&self.series, None, cx.admin);
            if !cols.is_empty() {
                at::section_title(ui, t, "Information");
                let focus = (self.focus.group == G_INFO).then_some(self.focus.index);
                for (i, r) in at::info_section(ui, t, &cols, focus).iter().enumerate() {
                    info_xs.push(r.rect.center().x);
                    if r.hovered() && moving {
                        self.focus = Focus { group: G_INFO, index: i };
                    }
                }
            }
            ui.add_space(t.row_gap);
        });

        let mut groups = vec![Group::default(); N_GROUPS];
        groups[G_HERO] = Group { len: hero_xs.len(), cols: hero_xs.len(), xs: hero_xs };
        if self.drop_open {
            groups[G_DROP] = Group { len: drop_xs.len(), cols: 1, xs: drop_xs };
        } else {
            groups[G_LIST] = Group { len: list_xs.len(), cols: list_xs.len(), xs: list_xs };
        }
        groups[G_NEXT] = Group { len: next_xs.len(), cols: next_xs.len(), xs: next_xs };
        groups[G_CAST] = Group { len: cast_xs.len(), cols: cast_xs.len(), xs: cast_xs };
        groups[G_INFO] = Group { len: info_xs.len(), cols: info_xs.len(), xs: info_xs };
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
        if let Some(p) = person {
            cx.push(super::person::screen(p.id, p.name));
        }
    }
}
