//! D2 gallery: the Apple TV details parts (`at_details`) arranged as a film page and a show page over test-server
//! titles, so the look can be judged before the real item / show screens are rewired. Opened with
//! `--screen gallery-details` (it appears after sign-in). Arrows move focus, Enter toggles More/Less and picks a
//! season, Info switches film ↔ show.
use super::at_details::{self as at, ActionBtn, EpisodeCard, HeroArt, RoundIcon};
use super::widgets;
use crate::api::{ticks_to_secs, ApiError, ImageKind, Item, ItemKind, SourceInfo};
use crate::app::{Cx, Screen};
use crate::nav::{move_focus, Focus, Group};
use crate::platform::Key;

const FILM: &str = "Copper Sky";
const SHOW: &str = "Night Shift Stories";

const G_MORE: usize = 0;
const G_ACT: usize = 1;
const G_CHIPS: usize = 2;
const G_SEASONS: usize = 3;
const G_EPS: usize = 4;
const G_NEXT: usize = 5;
const G_CAST: usize = 6;
const G_REL: usize = 7;
const G_INFO: usize = 8;
const N_GROUPS: usize = 9;

const OVERVIEW_LINES: usize = 3;

type Loaded = Result<(Item, Vec<SourceInfo>, Vec<Item>, Option<Item>), ApiError>;

pub struct GalleryDetails {
    show_page: bool,
    film: Option<(Item, Vec<SourceInfo>, Vec<Item>)>,
    show: Option<(Item, Vec<Item>, Option<Item>)>,
    season: i32,
    overview_open: bool,
    error: Option<String>,
    started: bool,
    focus: Focus,
    groups: Vec<Group>,
}

pub fn screen() -> Box<dyn Screen> {
    Box::new(GalleryDetails {
        show_page: false,
        film: None,
        show: None,
        season: 1,
        overview_open: false,
        error: None,
        started: false,
        focus: Focus { group: G_ACT, index: 0 },
        groups: vec![],
    })
}

fn find(c: &crate::api::Client, name: &str, kind: ItemKind) -> Result<Item, ApiError> {
    let hit = c.search(name)?.into_iter().find(|i| i.kind == kind && i.name == name);
    let hit = hit.ok_or_else(|| ApiError::Network(format!("{name} not on this server")))?;
    c.item(&hit.id)
}

impl GalleryDetails {
    fn load(&mut self, cx: &mut Cx) {
        if cx.client.is_none() {
            self.error = Some("Sign in first: this gallery opens after sign-in.".into());
            return;
        }
        let c = cx.api();
        cx.spawn("film", move || -> Loaded {
            let it = find(&c, FILM, ItemKind::Movie)?;
            let info = c.media_info(&it.id).unwrap_or_default();
            let similar = c.similar(&it.id, 12).unwrap_or_default();
            Ok((it, info, similar, None))
        });
        let c = cx.api();
        cx.spawn("show", move || -> Loaded {
            let it = find(&c, SHOW, ItemKind::Series)?;
            let eps = c.episodes(&it.id, None)?;
            let next = c.series_next_up(&it.id).unwrap_or(None);
            Ok((it, vec![], eps, next))
        });
    }

    /// The hero: backdrop, then (bottom-up) actions, overview + More, meta + badges, title, kicker.
    #[allow(clippy::too_many_arguments)]
    fn draw_hero(
        &mut self,
        ui: &mut egui::Ui,
        cx: &mut Cx,
        item: &Item,
        kicker: &str,
        meta: Vec<String>,
        badges: Vec<at::Badge>,
        btns: &[ActionBtn],
        groups: &mut [Group],
    ) {
        let t = cx.theme;
        let art = HeroArt {
            backdrop: at::tex(cx, ui, item, ImageKind::Backdrop, ui.ctx().screen_rect().width()),
            logo: at::logo(cx, ui, item),
        };
        let rect = at::hero(ui, t, art, t.backdrop_h);
        if matches!(self.focus.group, G_ACT | G_MORE) {
            ui.scroll_to_rect(rect.with_max_y(rect.top() + t.gap), None);
        }
        let mut col = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect.shrink2(egui::vec2(0.0, t.gap)))
                .layout(egui::Layout::bottom_up(egui::Align::Min)),
        );
        col.spacing_mut().item_spacing.y = 0.0;
        col.add_space(t.gap);
        let rs = at::action_row(&mut col, t, btns, (self.focus.group == G_ACT).then_some(self.focus.index));
        groups[G_ACT] = Group { len: rs.len(), cols: rs.len(), xs: rs.iter().map(|r| r.rect.center().x).collect() };
        for (i, r) in rs.iter().enumerate() {
            if r.hovered() && col.input(|i| i.pointer.is_moving()) {
                self.focus = Focus { group: G_ACT, index: i };
            }
        }
        col.add_space(t.gap);
        if let Some(ov) = item.overview.as_deref().map(str::trim).filter(|o| !o.is_empty()) {
            let long = at::overview_rows(&col, t, ov, t.hero_text_w) > OVERVIEW_LINES;
            if long {
                let label = if self.overview_open { "Less" } else { "More" };
                let r = at::text_button(&mut col, t, label, self.focus.group == G_MORE);
                groups[G_MORE] = Group { len: 1, cols: 1, xs: vec![r.rect.center().x] };
                if r.clicked() {
                    self.overview_open = !self.overview_open;
                }
            }
            at::overview(&mut col, t, ov, self.overview_open, OVERVIEW_LINES);
            col.add_space(t.gap / 2.0);
        }
        at::meta_row(&mut col, t, &meta, (None, None), &badges);
        col.add_space(t.gap / 2.0);
        at::hero_title(&mut col, t, &item.name, art.logo);
        if !kicker.is_empty() {
            col.add_space(t.gap / 4.0);
            at::kicker(&mut col, t, kicker);
        }
    }

    fn page_film(&mut self, ui: &mut egui::Ui, cx: &mut Cx, groups: &mut [Group]) {
        let Some((item, info, similar)) = self.film.clone() else { return };
        let t = cx.theme;
        let years = crate::story::year_line(&item, cx.settings.show_release_year, cx.settings.show_story_year);
        let facts: Vec<at::StreamFacts> = match info.first() {
            Some(src) => src.media_streams.iter().map(at::StreamFacts::from).collect(),
            None => item.media_streams.iter().map(at::StreamFacts::from).collect(),
        };
        let played = item.user_data.as_ref().is_some_and(|u| u.played);
        let fav = item.user_data.as_ref().is_some_and(|u| u.is_favorite);
        let resume = crate::ui::parts::resume_fraction(&item).is_some();
        let mut btns = vec![ActionBtn::Play(if resume { "Resume" } else { "Play" }.into())];
        if resume {
            btns.push(ActionBtn::Round { icon: RoundIcon::Restart, on: false, caption: "From start".into() });
        }
        btns.push(ActionBtn::Pill("Audio: English".into()));
        btns.push(ActionBtn::Round { icon: RoundIcon::Trailer, on: false, caption: "Trailer".into() });
        btns.push(ActionBtn::Round { icon: RoundIcon::Watched, on: played, caption: "Watched".into() });
        btns.push(ActionBtn::Round { icon: RoundIcon::Favourite, on: fav, caption: "Favourite".into() });
        btns.push(ActionBtn::Round { icon: RoundIcon::More, on: false, caption: "More".into() });
        self.draw_hero(ui, cx, &item, "", at::meta_parts(&item, &years), at::badges(&facts), &btns, groups);

        // link chips (genres, studio)
        let chips: Vec<String> =
            item.genres.iter().cloned().chain(item.studios.iter().map(|s| s.name.clone())).collect();
        if !chips.is_empty() {
            ui.add_space(t.gap / 2.0);
            let sel = vec![false; chips.len()];
            let out = widgets::chips(ui, t, &chips, &sel, (self.focus.group == G_CHIPS).then_some(self.focus.index));
            groups[G_CHIPS] = Group { len: chips.len(), cols: chips.len(), xs: out.xs.clone() };
        }
        self.cast_row(ui, cx, &item, groups);
        if !similar.is_empty() {
            at::section_title(ui, t, "Related");
            let h = t.poster_h + t.card_title_gap + t.body_size * 1.3;
            let focus = (self.focus.group == G_REL).then_some(self.focus.index);
            let rs = at::row(ui, t, "gd-related", similar.len(), h, focus, |ui, i| {
                let tx = at::tex(cx, ui, &similar[i], ImageKind::Primary, t.poster_w);
                at::poster_card(ui, t, tx, &similar[i].name, focus == Some(i))
            });
            self.track(&rs, G_REL, groups);
        }
        at::section_title(ui, t, "Information");
        let cols = at::info_columns(&item, info.first(), cx.admin);
        let rs = at::info_section(ui, t, &cols, (self.focus.group == G_INFO).then_some(self.focus.index));
        self.track(&rs, G_INFO, groups);
        ui.add_space(t.row_gap);
    }

    fn page_show(&mut self, ui: &mut egui::Ui, cx: &mut Cx, groups: &mut [Group]) {
        let Some((item, eps, next)) = self.show.clone() else { return };
        let t = cx.theme;
        let years = crate::story::year_line(&item, cx.settings.show_release_year, cx.settings.show_story_year);
        let mut seasons: Vec<i32> = eps.iter().filter_map(|e| e.parent_index_number).collect();
        seasons.sort_by_key(|&s| if s == 0 { i32::MAX } else { s });
        seasons.dedup();
        let mut meta = at::meta_parts(&Item { run_time_ticks: None, ..item.clone() }, &years);
        if !seasons.is_empty() {
            let n = seasons.iter().filter(|&&s| s > 0).count();
            meta.insert(meta.len().min(2), format!("{n} Season{}", if n == 1 { "" } else { "s" }));
        }
        let play = match next.as_ref().and_then(|n| Some((n.parent_index_number?, n.index_number?))) {
            Some((s, e)) => format!("Play S{s}, E{e}"),
            None => "Play".into(),
        };
        let played = item.user_data.as_ref().is_some_and(|u| u.played);
        let fav = item.user_data.as_ref().is_some_and(|u| u.is_favorite);
        let btns = vec![
            ActionBtn::Play(play),
            ActionBtn::Round { icon: RoundIcon::Shuffle, on: false, caption: "Shuffle".into() },
            ActionBtn::Round { icon: RoundIcon::Watched, on: played, caption: "Watched".into() },
            ActionBtn::Round { icon: RoundIcon::Favourite, on: fav, caption: "Favourite".into() },
            ActionBtn::Round { icon: RoundIcon::More, on: false, caption: "More".into() },
        ];
        self.draw_hero(ui, cx, &item, "", meta, vec![], &btns, groups);

        // Episodes: season picker (segmented pills) + a row of 16:9 episode cards
        at::section_title(ui, t, "Episodes");
        let labels: Vec<String> =
            seasons.iter().map(|&s| if s == 0 { "Specials".into() } else { format!("Season {s}") }).collect();
        let sel = seasons.iter().position(|&s| s == self.season).unwrap_or(0);
        let out = widgets::tabs(ui, t, &labels, sel, (self.focus.group == G_SEASONS).then_some(self.focus.index));
        groups[G_SEASONS] = Group { len: labels.len(), cols: labels.len(), xs: out.xs.clone() };
        if let Some(i) = out.clicked {
            self.season = seasons[i];
        }
        ui.add_space(t.gap / 2.0);
        let mut rows: Vec<&Item> = eps.iter().filter(|e| e.parent_index_number == Some(self.season)).collect();
        rows.sort_by_key(|e| e.index_number);
        let focus = (self.focus.group == G_EPS).then_some(self.focus.index);
        let rs = at::row(ui, t, "gd-eps", rows.len(), at::episode_card_h(t), focus, |ui, i| {
            let e = rows[i];
            let card = EpisodeCard {
                art: at::tex(cx, ui, e, ImageKind::Primary, t.ep_thumb_w),
                number: match e.index_number {
                    Some(n) if self.season > 0 => format!("Episode {n}"),
                    _ => "Special".into(),
                },
                title: e.name.clone(),
                overview: e.overview.clone().unwrap_or_default(),
                detail: e.run_time_ticks.map(|r| at::runtime_long(ticks_to_secs(r))).unwrap_or_default(),
                progress: crate::ui::parts::resume_fraction(e),
                played: e.user_data.as_ref().is_some_and(|u| u.played),
                unaired: false,
            };
            at::episode_card(ui, t, &card, focus == Some(i))
        });
        self.track(&rs, G_EPS, groups);

        if let Some(n) = next {
            at::section_title(ui, t, "Up Next");
            let focus = (self.focus.group == G_NEXT).then_some(self.focus.index);
            let rs = at::row(ui, t, "gd-next", 1, t.wide_h + t.card_title_gap + t.body_size * 1.3, focus, |ui, _| {
                let tx = at::tex(cx, ui, &n, ImageKind::Primary, t.wide_w);
                let sub = match (n.parent_index_number, n.index_number) {
                    (Some(s), Some(e)) => format!("S{s}, E{e} · {}", n.name),
                    _ => n.name.clone(),
                };
                at::wide_card(ui, t, tx, &item.name, &[&sub], crate::ui::parts::resume_fraction(&n), focus == Some(0))
            });
            self.track(&rs, G_NEXT, groups);
        }
        self.cast_row(ui, cx, &item, groups);
        at::section_title(ui, t, "Information");
        let cols = at::info_columns(&item, None, cx.admin);
        let rs = at::info_section(ui, t, &cols, (self.focus.group == G_INFO).then_some(self.focus.index));
        self.track(&rs, G_INFO, groups);
        ui.add_space(t.row_gap);
    }

    fn cast_row(&mut self, ui: &mut egui::Ui, cx: &mut Cx, item: &Item, groups: &mut [Group]) {
        if item.people.is_empty() {
            return;
        }
        let t = cx.theme;
        at::section_title(ui, t, "Cast & Crew");
        let focus = (self.focus.group == G_CAST).then_some(self.focus.index);
        let people = item.people.clone();
        let rs = at::row(ui, t, "gd-cast", people.len(), at::person_h(t), focus, |ui, i| {
            let p = &people[i];
            let role = at::role_of(p);
            let tx = at::person_tex(cx, ui, p);
            at::person_circle(ui, t, tx, &p.name, &role, focus == Some(i))
        });
        self.track(&rs, G_CAST, groups);
    }

    /// Records a row's focus group and moves focus to a hovered cell.
    fn track(&mut self, rs: &[egui::Response], g: usize, groups: &mut [Group]) {
        groups[g] = Group { len: rs.len(), cols: rs.len(), xs: rs.iter().map(|r| r.rect.center().x).collect() };
        for (i, r) in rs.iter().enumerate() {
            if r.hovered() && r.ctx.input(|i| i.pointer.is_moving()) {
                self.focus = Focus { group: g, index: i };
            }
        }
    }
}

impl Screen for GalleryDetails {
    fn name(&self) -> &'static str {
        "gallery-details"
    }

    fn on_key(&mut self, _cx: &mut Cx, key: Key) -> bool {
        match key {
            Key::Info => {
                self.show_page = !self.show_page;
                self.focus = Focus { group: G_ACT, index: 0 };
                self.overview_open = false;
                true
            }
            Key::Enter => {
                match self.focus.group {
                    G_MORE => self.overview_open = !self.overview_open,
                    G_SEASONS => {
                        if let Some((_, eps, _)) = &self.show {
                            let mut s: Vec<i32> = eps.iter().filter_map(|e| e.parent_index_number).collect();
                            s.sort_by_key(|&s| if s == 0 { i32::MAX } else { s });
                            s.dedup();
                            if let Some(&v) = s.get(self.focus.index) {
                                self.season = v;
                            }
                        }
                    }
                    _ => {}
                }
                true
            }
            Key::Up | Key::Down | Key::Left | Key::Right => {
                self.focus = move_focus(&self.groups, self.focus, key);
                true
            }
            _ => false,
        }
    }

    fn on_done(&mut self, cx: &mut Cx, tag: &'static str, payload: Box<dyn std::any::Any + Send>) {
        let Ok(r) = payload.downcast::<Loaded>() else { return };
        match (*r, tag) {
            (Ok((it, info, similar, _)), "film") => self.film = Some((it, info, similar)),
            (Ok((it, _, eps, next)), "show") => {
                if let Some(s) = next.as_ref().and_then(|n| n.parent_index_number) {
                    self.season = s;
                }
                self.show = Some((it, eps, next));
            }
            (Err(ApiError::Unauthorized), _) => cx.signed_out(),
            (Err(e), _) => self.error = Some(e.to_string()),
            _ => {}
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        if !self.started {
            self.started = true;
            self.load(cx);
        }
        let mut groups = vec![Group::default(); N_GROUPS];
        egui::ScrollArea::vertical().id_salt("gallery-details").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            if let Some(e) = &self.error {
                super::parts::error_line(ui, cx.theme, e);
            }
            if self.show_page {
                self.page_show(ui, cx, &mut groups);
            } else {
                self.page_film(ui, cx, &mut groups);
            }
        });
        self.groups = groups;
        self.focus = move_focus(&self.groups, self.focus, Key::Enter);
    }
}
