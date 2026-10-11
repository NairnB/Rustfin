//! R-UI-5 Library: title + count, tabs (G11: Suggestions / Genres / Studios / Upcoming / Episodes / Collections),
//! a sort menu + view mode + filter chips + a "Filters…" dialog (P-LIB-2..14), an A–Z jump bar (P-LIB-13), then a
//! stack of bordered sections (R-F2-1), each a full grid of posters, thumbs or list rows. All pages load in the
//! background (100 at a time); the first page is drawn at once. Story order and My order are client-side sorts over
//! all loaded items (R-F3-2, R-F4-1).
use super::{parts, widgets};
use crate::api::{ApiError, ExtraFilters, FilterOptions, Item, Page, Recommendation, Result, SortBy};
use crate::app::{Cx, Screen};
use crate::nav::{self, Focus, Group};
use crate::order;
use crate::platform::Key;
use crate::sections;
use crate::settings::{LibraryView, ViewMode};
use crate::story;
use crate::tags;
use crate::theme::Theme;

const PAGE: usize = 100;

const G_TABS: usize = 0;
const G_DROP: usize = 1;
const G_TOGGLE: usize = 2;
const G_CHIPS: usize = 3;
const G_PLAY: usize = 4;
const G_AZ: usize = 5;
const G_SECTIONS: usize = 6;

/// P-LIB-23: how many items Play all / Shuffle queue.
const PLAY_LIMIT: usize = 300;

const CHIP_LABELS: [&str; 4] = ["Unplayed", "Played", "In progress", "Favourites"];

/// P-LIB-13: the jump bar's letters ('#' = names that do not start with a letter).
const AZ: [char; 27] = [
    '#', 'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M', 'N', 'O', 'P', 'Q', 'R', 'S', 'T', 'U', 'V',
    'W', 'X', 'Y', 'Z',
];

/// P-LIB-10..12: the on/off feature flags of the "Filters…" dialog, in display order.
const FEATURES: [&str; 8] = ["Subtitles", "Trailer", "Special features", "Theme song", "Theme video", "HD", "4K", "3D"];

fn feature_flag(x: &mut ExtraFilters, i: usize) -> Option<&mut bool> {
    Some(match i {
        0 => &mut x.has_subtitles,
        1 => &mut x.has_trailer,
        2 => &mut x.has_special_features,
        3 => &mut x.has_theme_song,
        4 => &mut x.has_theme_video,
        5 => &mut x.hd,
        6 => &mut x.uhd,
        7 => &mut x.three_d,
        _ => return None,
    })
}

/// The jump-bar letter an item sorts under: the first letter of its name without a leading "The " / "A " / "An "
/// (the server's own SortName rule), upper-cased; '#' for anything else.
pub fn letter_of(name: &str) -> char {
    let lower = name.trim().to_lowercase();
    let rest = ["the ", "a ", "an "].iter().find_map(|a| lower.strip_prefix(a)).unwrap_or(&lower);
    match rest.chars().next() {
        Some(c) if c.is_alphabetic() => c.to_uppercase().next().unwrap_or('#'),
        _ => '#',
    }
}

/// P-LIB-13: the position of the first item whose name sorts under `ch`.
fn first_with_letter(items: &[Item], ch: char) -> Option<usize> {
    items.iter().position(|it| letter_of(&it.name) == ch)
}

/// Which tabs a library shows: movies get Suggestions + Collections, shows get Studios / Upcoming / Episodes.
fn tabs_for(collection_type: Option<&str>) -> Vec<Tab> {
    match collection_type {
        Some("movies") => vec![Tab::Library, Tab::Suggestions, Tab::Genres, Tab::Collections],
        Some("tvshows") => vec![Tab::Library, Tab::Genres, Tab::Studios, Tab::Upcoming, Tab::Episodes],
        _ => vec![Tab::Library, Tab::Genres],
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Library,
    Suggestions,
    Genres,
    Studios,
    Upcoming,
    Episodes,
    Collections,
}

impl Tab {
    fn label(self) -> &'static str {
        match self {
            Tab::Library => "Library",
            Tab::Suggestions => "Suggestions",
            Tab::Genres => "Genres",
            Tab::Studios => "Studios",
            Tab::Upcoming => "Upcoming",
            Tab::Episodes => "Episodes",
            Tab::Collections => "Collections",
        }
    }
    /// The tabs that page through the library's own items (sort, filters, A–Z, view modes apply).
    fn is_items(self) -> bool {
        matches!(self, Tab::Library | Tab::Episodes)
    }
}

/// What a non-item tab loaded.
enum Side {
    Recs(Vec<Recommendation>),
    Names(Vec<Item>),
    Items(Vec<Item>),
}

/// The "Filters…" dialog (P-LIB-10..12): a draft of the extra filters, applied with the Apply button.
struct FilterDlg {
    draft: ExtraFilters,
    focus: usize,
    /// Screen rects of every focusable thing, from the last frame (chips, then Clear and Apply).
    rects: Vec<egui::Rect>,
    scroll_to_focus: bool,
}

/// One focusable thing in the dialog.
#[derive(Clone, PartialEq)]
enum DlgItem {
    Genre(String),
    Rating(String),
    Tag(String),
    YearFrom,
    YearTo,
    Feature(usize),
    Clear,
    Apply,
}

/// Up/Down by screen position, Left/Right by order; `rects` are the focusable things in reading order.
fn dlg_move(rects: &[egui::Rect], cur: usize, key: Key) -> usize {
    if rects.is_empty() {
        return 0;
    }
    let cur = cur.min(rects.len() - 1);
    match key {
        Key::Left => cur.saturating_sub(1),
        Key::Right => (cur + 1).min(rects.len() - 1),
        Key::Up | Key::Down => {
            let c = rects[cur].center();
            let up = key == Key::Up;
            let row_y = rects
                .iter()
                .map(|r| r.center().y)
                .filter(|&y| if up { y < c.y - 1.0 } else { y > c.y + 1.0 })
                .fold(None, |best: Option<f32>, y| match best {
                    Some(b) if (up && b >= y) || (!up && b <= y) => Some(b),
                    _ => Some(y),
                });
            let Some(y) = row_y else { return cur };
            rects
                .iter()
                .enumerate()
                .filter(|(_, r)| (r.center().y - y).abs() < 1.0)
                .min_by(|a, b| (a.1.center().x - c.x).abs().total_cmp(&(b.1.center().x - c.x).abs()))
                .map(|(i, _)| i)
                .unwrap_or(cur)
        }
        _ => cur,
    }
}

/// F4 (R-F4-2): the card being moved in My order — `from` and `to` are indices into the ordered list.
#[derive(Clone, Copy)]
struct MoveState {
    from: usize,
    to: usize,
}

/// F4 (R-F4-2): an in-progress mouse drag (ghost follows the pointer).
#[derive(Clone, Copy)]
struct DragState {
    from: usize,
    to: usize,
    ptr: egui::Pos2,
    moved: bool,
}

/// Geometry of one section's grid, enough to place a drop marker or the dragged card's ghost.
struct GridGeom {
    left: f32,
    top: f32,
    cols: usize,
    cell: egui::Vec2,
    size: egui::Vec2,
}

/// The card rect (poster + label) at a section-relative index, matching `parts::grid`'s layout.
fn card_rect(g: &GridGeom, t: &Theme, i: usize) -> egui::Rect {
    let cols = g.cols.max(1);
    let r = i / cols;
    let c = i % cols;
    let min = egui::pos2(g.left + c as f32 * g.cell.x, g.top + r as f32 * g.cell.y);
    egui::Rect::from_min_size(min, egui::vec2(g.size.x, g.size.y + t.card_label_h))
}

pub struct Library {
    view: Item,
    items: Vec<Option<Item>>,
    total: usize,
    next_start: usize,
    in_flight: bool,
    error: Option<String>,
    started: bool,
    prefs: LibraryView,
    play_loading: bool,
    dropdown: widgets::DropdownState,
    view_dd: widgets::DropdownState,
    sort_kinds: Vec<SortBy>,
    gen: u64,
    focus: Focus,
    groups: Vec<Group>,
    // F4 (R-F4-2): My-order editing (admin only).
    moving: Option<MoveState>,
    drag: Option<DragState>,
    move_error: Option<String>,
    // G11: tabs, the non-item tabs' data, the filters dialog and the A–Z jump bar.
    tab: Tab,
    tabs: Vec<Tab>,
    side: Option<Result<Side>>,
    side_gen: u64,
    fopts: Option<FilterOptions>,
    dlg: Option<FilterDlg>,
    jump: Option<char>,
    az_on: bool,
}

pub fn screen(view: Item) -> Box<dyn Screen> {
    // G7 (P-LIB-17): the server's own "Collections" library holds BoxSets, not films, so it opens the collections grid.
    if view.collection_type.as_deref() == Some("boxsets") {
        return super::collection::list();
    }
    // G7 (P-COLL-4): likewise the "Playlists" library lists the user's playlists.
    if view.collection_type.as_deref() == Some("playlists") {
        return super::playlists::list();
    }
    let tabs = tabs_for(view.collection_type.as_deref());
    Box::new(Library {
        view,
        items: vec![],
        total: 0,
        next_start: 0,
        in_flight: false,
        error: None,
        started: false,
        prefs: LibraryView::default(),
        play_loading: false,
        dropdown: widgets::DropdownState::default(),
        view_dd: widgets::DropdownState::default(),
        sort_kinds: vec![],
        gen: 0,
        focus: Focus { group: G_DROP, index: 0 },
        groups: vec![],
        moving: None,
        drag: None,
        move_error: None,
        tab: Tab::Library,
        tabs,
        side: None,
        side_gen: 0,
        fopts: None,
        dlg: None,
        jump: None,
        az_on: false,
    })
}

impl Library {
    fn request_page(&mut self, cx: &Cx) {
        if self.in_flight {
            return;
        }
        let client = cx.api();
        let view_id = self.view.id.clone();
        let start = self.next_start;
        let q = self.prefs.query();
        let gen = self.gen;
        let types = if self.tab == Tab::Episodes { "Episode" } else { "Movie,Series" };
        self.in_flight = true;
        cx.spawn("page", move || (gen, client.library_query_types(&view_id, start, PAGE, &q, types)));
    }

    /// Restart the load from page 0 with the current sort + filters (gen invalidates any in-flight page).
    fn reload(&mut self, cx: &Cx) {
        self.error = None;
        self.total = 0;
        self.next_start = 0;
        self.items.clear();
        self.in_flight = false;
        self.gen += 1;
        self.request_page(cx);
    }

    fn persist(&mut self, cx: &mut Cx) {
        cx.settings.library_view.insert(self.view.id.clone(), self.prefs.clone());
        cx.save_settings();
    }

    /// `IncludeItemTypes` for the Genres / Studios tabs and the filter options.
    fn kinds(&self) -> &'static str {
        match self.view.collection_type.as_deref() {
            Some("movies") => "Movie",
            Some("tvshows") => "Series",
            _ => "Movie,Series",
        }
    }

    /// The view mode actually drawn: the Episodes tab has no posters, so Posters shows thumbs there.
    fn mode(&self) -> ViewMode {
        if self.tab == Tab::Episodes && self.prefs.mode == ViewMode::Posters {
            ViewMode::Thumbs
        } else {
            self.prefs.mode
        }
    }

    /// Switch tab: Collections opens G7's grid; the item tabs reload the library; the rest load their own data.
    fn set_tab(&mut self, cx: &mut Cx, tab: Tab) {
        if tab == Tab::Collections {
            cx.push(super::collection::list());
            return;
        }
        if tab == self.tab {
            return;
        }
        self.tab = tab;
        self.focus = Focus { group: G_TABS, index: self.tabs.iter().position(|&x| x == tab).unwrap_or(0) };
        self.dropdown.open = false;
        self.view_dd.open = false;
        if tab.is_items() {
            self.side = None;
            self.reload(cx);
        } else {
            self.items.clear();
            self.total = 0;
            self.next_start = 0;
            self.in_flight = false;
            self.error = None;
            self.gen += 1;
            self.load_side(cx);
        }
    }

    /// Load the data of the current non-item tab (P-LIB-16, 18..20).
    fn load_side(&mut self, cx: &Cx) {
        self.side = None;
        self.side_gen += 1;
        let gen = self.side_gen;
        let client = cx.api();
        let id = self.view.id.clone();
        let kinds = self.kinds();
        let tab = self.tab;
        cx.spawn("tab", move || {
            let r: Result<Side> = match tab {
                Tab::Suggestions => client.recommendations(&id).map(Side::Recs),
                Tab::Genres => client.genres(&id, kinds).map(Side::Names),
                Tab::Studios => client.studios(&id, kinds).map(Side::Names),
                Tab::Upcoming => client.upcoming(&id).map(Side::Items),
                _ => Ok(Side::Items(vec![])),
            };
            (gen, r)
        });
    }

    /// A genre or studio picked on its tab becomes the library filter, and the Library tab shows the result.
    fn apply_name(&mut self, cx: &mut Cx, it: Item) {
        match self.tab {
            Tab::Genres => self.prefs.extra.genres = vec![it.name],
            Tab::Studios => self.prefs.extra.studio = Some((it.id, it.name)),
            _ => return,
        }
        self.persist(cx);
        self.set_tab(cx, Tab::Library);
    }

    /// Enter / click on a focused thing in the body (a card, a name, Retry).
    fn activate(&mut self, cx: &mut Cx, f: Focus) {
        if f.group < G_SECTIONS {
            return;
        }
        if self.tab.is_items() {
            if let Some(item) = self.item_at(f) {
                cx.push(super::item::screen_in(item, Some(tags::place_id(&self.view.id))));
            }
            return;
        }
        match &self.side {
            Some(Err(_)) => self.load_side(cx),
            Some(Ok(Side::Names(v))) => {
                if let Some(it) = v.get(f.index).cloned() {
                    self.apply_name(cx, it);
                }
            }
            Some(Ok(_)) => {
                if let Some(it) = self.side_item(f) {
                    cx.push(super::item::screen_in(it, None));
                }
            }
            None => {}
        }
    }

    /// The item under a focus on a Suggestions / Upcoming tab.
    fn side_item(&self, f: Focus) -> Option<Item> {
        match self.side.as_ref()?.as_ref().ok()? {
            Side::Recs(r) => r.get(f.group.checked_sub(G_SECTIONS)?)?.items.get(f.index).cloned(),
            Side::Items(v) | Side::Names(v) => v.get(f.index).cloned(),
        }
    }

    /// The loaded items in the chosen order. Server sorts already come back ordered; Story/My order re-sort the
    /// loaded items locally (the library loads every page in the background, so this converges to the full list).
    fn ordered_loaded(&self) -> Vec<Item> {
        let mut v: Vec<Item> = self.items.iter().filter_map(|o| o.clone()).collect();
        match self.prefs.sort {
            SortBy::StoryOrder => story::sort_story(&mut v),
            SortBy::MyOrder => order::sort_my_order(&mut v, &tags::place_id(&self.view.id)),
            _ => {}
        }
        v
    }

    /// The bordered sections shown (R-F2-1); the Episodes tab is one plain list.
    fn display_sections(&self, ordered: &[Item]) -> Vec<(String, Vec<Item>)> {
        if self.tab == Tab::Episodes {
            return if ordered.is_empty() { vec![] } else { vec![("Episodes".to_string(), ordered.to_vec())] };
        }
        sections::group(ordered).into_iter().map(|(s, v)| (s.label().to_string(), v)).collect()
    }

    /// The sort menu options for the currently loaded items: server sorts, then Story order when any item has a
    /// story year, then My order.
    fn sort_options(&self, loaded: &[Item]) -> Vec<SortBy> {
        let mut kinds = vec![
            SortBy::Name,
            SortBy::DateAdded,
            SortBy::PremiereDate,
            SortBy::CommunityRating,
            SortBy::CriticRating,
            SortBy::DatePlayed,
            SortBy::PlayCount,
            SortBy::Runtime,
            SortBy::ParentalRating,
            SortBy::Random,
        ];
        if story::any_story(loaded) || self.prefs.sort == SortBy::StoryOrder {
            kinds.push(SortBy::StoryOrder);
        }
        kinds.push(SortBy::MyOrder);
        kinds
    }

    fn pick_sort(&mut self, cx: &mut Cx, i: usize) {
        let Some(&sort) = self.sort_kinds.get(i) else { return };
        if self.prefs.sort == sort {
            return;
        }
        self.prefs.sort = sort;
        self.persist(cx);
        self.reload(cx);
    }

    /// P-LIB-14: pick Posters / Thumbs / List.
    fn pick_view(&mut self, cx: &mut Cx, i: usize) {
        let Some(&mode) = ViewMode::ALL.get(i) else { return };
        if self.prefs.mode != mode {
            self.prefs.mode = mode;
            self.persist(cx);
        }
    }

    fn toggle_filter(&mut self, cx: &mut Cx, i: usize) {
        match i {
            0 => self.prefs.filters.unplayed = !self.prefs.filters.unplayed,
            1 => self.prefs.filters.played = !self.prefs.filters.played,
            2 => self.prefs.filters.resumable = !self.prefs.filters.resumable,
            3 => self.prefs.filters.favourites = !self.prefs.filters.favourites,
            _ => return,
        }
        self.persist(cx);
        self.reload(cx);
    }

    /// P-LIB-23: queue the first `PLAY_LIMIT` playable items for the current sort + filters.
    fn play_library(&mut self, cx: &mut Cx, shuffle: bool) {
        if self.play_loading {
            return;
        }
        self.play_loading = true;
        let client = cx.api();
        let view_id = self.view.id.clone();
        let q = self.prefs.query();
        cx.spawn("playall", move || client.library_playable(&view_id, PLAY_LIMIT, &q, shuffle));
    }

    /// Item under a section-relative focus, using the current ordering.
    fn item_at(&self, f: Focus) -> Option<Item> {
        if f.group < G_SECTIONS {
            return None;
        }
        let sections = self.display_sections(&self.ordered_loaded());
        sections.get(f.group - G_SECTIONS).and_then(|(_, v)| v.get(f.index)).cloned()
    }

    /// The flat (ordered-list) index of the currently focused card, if the focus is on a section grid.
    fn focused_flat_index(&self) -> Option<usize> {
        if self.focus.group < G_SECTIONS {
            return None;
        }
        let sections = self.display_sections(&self.ordered_loaded());
        let mut off = 0;
        for (gi, (_, v)) in sections.iter().enumerate() {
            if gi == self.focus.group - G_SECTIONS {
                return (self.focus.index < v.len()).then_some(off + self.focus.index);
            }
            off += v.len();
        }
        None
    }

    /// The item at a flat index of the ordered list.
    fn flat_item(&self, flat: usize) -> Option<Item> {
        self.ordered_loaded().get(flat).cloned()
    }

    /// Apply a set of `move_writes` results to the local item list immediately, so the grid re-sorts at once
    /// (the background writes converge to the same state).
    fn apply_writes(&mut self, writes: &[(String, u32)]) {
        let place = tags::place_id(&self.view.id);
        for (id, n) in writes {
            for slot in self.items.iter_mut().flatten() {
                if slot.id == *id {
                    slot.tags = tags::with_order(&slot.tags, &place, Some(*n));
                }
            }
        }
    }

    /// Compute the writes for a move and write them in the background; the grid already shows the new order.
    fn drop_move(&mut self, cx: &mut Cx, from: usize, to: usize) {
        self.move_error = None;
        if from == to {
            return;
        }
        let ordered = self.ordered_loaded();
        let place = tags::place_id(&self.view.id);
        let writes = order::move_writes(&ordered, &place, from, to);
        if writes.is_empty() {
            return;
        }
        self.apply_writes(&writes);
        let client = cx.api();
        cx.spawn("order", move || {
            for (id, n) in &writes {
                client.update_tags(id, |t| tags::with_order(t, &place, Some(*n)))?;
            }
            Ok::<(), ApiError>(())
        });
    }

    /// Draw the F4 move/drag visuals for one section: a dimmed lifted card, and a drop marker at the target gap.
    fn draw_move_visuals(&mut self, ui: &mut egui::Ui, cx: &mut Cx, g: &GridGeom, off: usize, len: usize) {
        let t = cx.theme;
        let lift = |ui: &egui::Ui, i: usize| {
            let r = card_rect(g, t, i);
            ui.painter().rect_filled(r, t.radius, t.ghost_scrim);
        };
        if let Some(m) = self.moving {
            if m.from >= off && m.from < off + len {
                lift(ui, m.from - off);
            }
            if m.to != m.from && m.to >= off && m.to < off + len {
                let r = card_rect(g, t, m.to - off);
                ui.scroll_to_rect(r.expand(t.gap), None);
                parts::drop_marker(ui, t, r.left() - t.gap / 2.0, r.top(), t.poster_h);
            }
        }
        if let Some(d) = self.drag {
            if d.from >= off && d.from < off + len {
                lift(ui, d.from - off);
            }
            if d.to != d.from && d.to >= off && d.to < off + len {
                let r = card_rect(g, t, d.to - off);
                parts::drop_marker(ui, t, r.left() - t.gap / 2.0, r.top(), t.poster_h);
            }
        }
    }

    // ---------- the "Filters…" dialog (P-LIB-10..12) ----------

    fn open_filters(&mut self, cx: &Cx) {
        if self.fopts.is_none() {
            let client = cx.api();
            let id = self.view.id.clone();
            let kinds = self.kinds();
            cx.spawn("fopts", move || client.filter_options(&id, kinds));
        }
        self.dlg = Some(FilterDlg { draft: self.prefs.extra.clone(), focus: 0, rects: vec![], scroll_to_focus: false });
    }

    /// Everything focusable in the dialog, in drawing order.
    fn dlg_items(&self) -> Vec<DlgItem> {
        let mut v = vec![];
        if let Some(o) = &self.fopts {
            v.extend(o.genres.iter().cloned().map(DlgItem::Genre));
            v.extend(o.official_ratings.iter().cloned().map(DlgItem::Rating));
            v.extend(o.tags.iter().filter(|t| !t.starts_with("rf-")).cloned().map(DlgItem::Tag));
        }
        v.push(DlgItem::YearFrom);
        v.push(DlgItem::YearTo);
        v.extend((0..FEATURES.len()).map(DlgItem::Feature));
        v.push(DlgItem::Clear);
        v.push(DlgItem::Apply);
        v
    }

    fn dlg_key(&mut self, cx: &mut Cx, key: Key) -> bool {
        let items = self.dlg_items();
        let Some(d) = self.dlg.as_mut() else { return false };
        match key {
            Key::Back => {
                self.dlg = None;
            }
            Key::Left | Key::Right | Key::Up | Key::Down => {
                d.focus = dlg_move(&d.rects, d.focus, key);
                d.scroll_to_focus = true;
            }
            Key::Enter => {
                let it = items.get(d.focus).cloned();
                self.dlg_activate(cx, it);
            }
            Key::Text(c) if c.is_ascii_digit() => match items.get(d.focus) {
                Some(DlgItem::YearFrom) => d.draft.year_from = year_type(d.draft.year_from, c),
                Some(DlgItem::YearTo) => d.draft.year_to = year_type(d.draft.year_to, c),
                _ => {}
            },
            Key::Backspace => match items.get(d.focus) {
                Some(DlgItem::YearFrom) => d.draft.year_from = year_back(d.draft.year_from),
                Some(DlgItem::YearTo) => d.draft.year_to = year_back(d.draft.year_to),
                _ => {}
            },
            _ => {}
        }
        true
    }

    fn dlg_activate(&mut self, cx: &mut Cx, it: Option<DlgItem>) {
        let Some(d) = self.dlg.as_mut() else { return };
        match it {
            Some(DlgItem::Genre(g)) => toggle(&mut d.draft.genres, &g),
            Some(DlgItem::Rating(r)) => toggle(&mut d.draft.ratings, &r),
            Some(DlgItem::Tag(t)) => toggle(&mut d.draft.tags, &t),
            Some(DlgItem::Feature(i)) => {
                if let Some(f) = feature_flag(&mut d.draft, i) {
                    *f = !*f;
                }
            }
            Some(DlgItem::Clear) => d.draft = ExtraFilters::default(),
            Some(DlgItem::Apply) => {
                let mut x = d.draft.clone();
                x.year_from = x.year_from.filter(|&y| y > 0);
                x.year_to = x.year_to.filter(|&y| y > 0);
                self.dlg = None;
                if x != self.prefs.extra {
                    self.prefs.extra = x;
                    self.persist(cx);
                    self.reload(cx);
                }
            }
            Some(DlgItem::YearFrom) | Some(DlgItem::YearTo) | None => {}
        }
    }

    fn dialog_ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx, pointer_moved: bool) {
        let Some(mut d) = self.dlg.take() else { return };
        let t = cx.theme;
        let ctx = ui.ctx().clone();
        let screen = ctx.screen_rect();
        ctx.layer_painter(egui::LayerId::new(egui::Order::Middle, egui::Id::new("g11-scrim")))
            .rect_filled(screen, 0.0, t.scrim);
        let items = self.dlg_items();
        let loading = self.fopts.is_none();
        let mut rects = vec![egui::Rect::NOTHING; items.len()];
        let mut clicked: Option<usize> = None;
        let mut hovered: Option<usize> = None;
        let size = egui::vec2(
            t.filters_w.min(screen.width() - 2.0 * t.margin),
            t.filters_h.min(screen.height() - 2.0 * t.margin),
        );
        egui::Area::new(egui::Id::new("g11-filters"))
            .order(egui::Order::Foreground)
            .fade_in(false)
            .fixed_pos(screen.center())
            .pivot(egui::Align2::CENTER_CENTER)
            .show(&ctx, |ui| {
                let (panel, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                ui.painter().rect_filled(panel, t.radius, t.surface);
                let inner = panel.shrink(t.dialog_pad);
                ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
                    parts::heading(ui, t, "Filters");
                    ui.add_space(t.gap / 2.0);
                    if loading {
                        parts::muted(ui, t, "Loading…");
                    }
                    let body_h = inner.height() - t.heading_size - t.gap * 1.5 - t.button_h - t.gap;
                    egui::ScrollArea::vertical()
                        .id_salt("g11-filters-body")
                        .max_height(body_h)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            let mut i = 0;
                            while i < items.len() {
                                let Some(head) = dlg_section(&items[i]) else { break };
                                let start = i;
                                while i < items.len() && dlg_section(&items[i]) == Some(head) {
                                    i += 1;
                                }
                                parts::muted(ui, t, head);
                                ui.add_space(t.gap / 4.0);
                                ui.horizontal_wrapped(|ui| {
                                    ui.spacing_mut().item_spacing = egui::vec2(t.gap / 2.0, t.gap / 2.0);
                                    for (k, it) in items.iter().enumerate().take(i).skip(start) {
                                        let focused = d.focus == k;
                                        let resp = match it {
                                            DlgItem::Genre(g) => {
                                                dlg_chip(ui, t, g, d.draft.genres.contains(g), focused)
                                            }
                                            DlgItem::Rating(r) => {
                                                dlg_chip(ui, t, r, d.draft.ratings.contains(r), focused)
                                            }
                                            DlgItem::Tag(g) => dlg_chip(ui, t, g, d.draft.tags.contains(g), focused),
                                            DlgItem::Feature(f) => {
                                                let on = feature_on(&d.draft, *f);
                                                dlg_chip(ui, t, FEATURES[*f], on, focused)
                                            }
                                            DlgItem::YearFrom => {
                                                parts::muted(ui, t, "From");
                                                year_field(ui, t, d.draft.year_from, focused)
                                            }
                                            DlgItem::YearTo => {
                                                parts::muted(ui, t, "To");
                                                year_field(ui, t, d.draft.year_to, focused)
                                            }
                                            DlgItem::Clear | DlgItem::Apply => continue,
                                        };
                                        rects[k] = resp.rect;
                                        if focused && d.scroll_to_focus {
                                            resp.scroll_to_me(None);
                                        }
                                        if resp.hovered() {
                                            hovered = Some(k);
                                        }
                                        if resp.clicked() {
                                            clicked = Some(k);
                                        }
                                    }
                                });
                                ui.add_space(t.gap / 2.0);
                            }
                        });
                    ui.add_space(t.gap / 2.0);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = t.gap / 2.0;
                        let n = items.len();
                        for (k, label, primary) in [(n - 2, "Clear all", false), (n - 1, "Apply", true)] {
                            let r = parts::button(ui, t, label, d.focus == k, primary);
                            rects[k] = r.rect;
                            if r.hovered() {
                                hovered = Some(k);
                            }
                            if r.clicked() {
                                clicked = Some(k);
                            }
                        }
                    });
                });
            });
        d.rects = rects;
        d.scroll_to_focus = false;
        if pointer_moved {
            if let Some(h) = hovered {
                d.focus = h;
            }
        }
        if let Some(k) = clicked {
            d.focus = k;
        }
        self.dlg = Some(d);
        if let Some(k) = clicked {
            self.dlg_activate(cx, items.get(k).cloned());
        }
    }

    // ---------- bodies ----------

    /// The Suggestions / Genres / Studios / Upcoming tabs. Returns the clicked focus.
    fn side_body(
        &mut self,
        ui: &mut egui::Ui,
        cx: &mut Cx,
        groups: &mut Vec<Group>,
        hovered: &mut Option<Focus>,
    ) -> Option<Focus> {
        let t = cx.theme;
        let focus = self.focus;
        let mut clicked: Option<Focus> = None;
        let selected: Vec<String> = match self.tab {
            Tab::Genres => self.prefs.extra.genres.clone(),
            Tab::Studios => self.prefs.extra.studio.iter().map(|(_, n)| n.clone()).collect(),
            _ => vec![],
        };
        let tab = self.tab;
        match &self.side {
            None => {
                ui.horizontal(|ui| {
                    for _ in 0..6 {
                        parts::skeleton_card(ui, t, parts::CardKind::Poster);
                    }
                });
            }
            Some(Err(e)) => {
                parts::error_line(ui, t, &e.to_string());
                let r = parts::button(ui, t, "Retry", focus.group == G_SECTIONS, false);
                groups.push(Group { len: 1, cols: 1, xs: vec![r.rect.center().x] });
                if r.hovered() {
                    *hovered = Some(Focus { group: G_SECTIONS, index: 0 });
                }
                if r.clicked() {
                    clicked = Some(Focus { group: G_SECTIONS, index: 0 });
                }
            }
            Some(Ok(Side::Names(names))) => {
                if names.is_empty() {
                    parts::muted(ui, t, if tab == Tab::Studios { "No studios yet." } else { "No genres yet." });
                    groups.push(Group::default());
                } else {
                    let w = t.dropdown_w;
                    let cols = nav::grid_cols(ui.available_width(), 0.0, w, t.gap / 2.0);
                    let mut xs = vec![];
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for (ri, row) in names.chunks(cols).enumerate() {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = t.gap / 2.0;
                                for (ci, it) in row.iter().enumerate() {
                                    let i = ri * cols + ci;
                                    let on = selected.contains(&it.name);
                                    let r = parts::button_width(
                                        ui,
                                        t,
                                        &it.name,
                                        focus.group == G_SECTIONS && focus.index == i,
                                        on,
                                        w,
                                    );
                                    xs.push(r.rect.center().x);
                                    if focus.group == G_SECTIONS && focus.index == i {
                                        r.scroll_to_me(None);
                                    }
                                    if r.hovered() {
                                        *hovered = Some(Focus { group: G_SECTIONS, index: i });
                                    }
                                    if r.clicked() {
                                        clicked = Some(Focus { group: G_SECTIONS, index: i });
                                    }
                                }
                            });
                        }
                    });
                    groups.push(Group { len: names.len(), cols, xs });
                }
            }
            Some(Ok(Side::Recs(recs))) => {
                if recs.iter().all(|r| r.items.is_empty()) {
                    parts::muted(ui, t, "No suggestions yet. Watch a few films and they will appear here.");
                }
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (ci, rec) in recs.iter().enumerate() {
                        let gi = G_SECTIONS + ci;
                        if rec.items.is_empty() {
                            groups.push(Group::default());
                            continue;
                        }
                        let out = parts::row(
                            ui,
                            cx,
                            &format!("g11-rec-{ci}"),
                            &rec.title(),
                            &rec.items,
                            parts::CardKind::Poster,
                            (focus.group == gi).then_some(focus.index),
                        );
                        groups.push(Group { len: rec.items.len(), cols: rec.items.len(), xs: out.xs });
                        if let Some(i) = out.hovered {
                            *hovered = Some(Focus { group: gi, index: i });
                        }
                        if let Some(i) = out.clicked {
                            clicked = Some(Focus { group: gi, index: i });
                        }
                    }
                });
            }
            Some(Ok(Side::Items(items))) => {
                if items.is_empty() {
                    parts::muted(ui, t, "Nothing upcoming right now.");
                    groups.push(Group::default());
                } else {
                    let cols = nav::grid_cols(ui.available_width(), 0.0, t.thumb_w, t.gap);
                    let opts: Vec<Option<Item>> = items.iter().cloned().map(Some).collect();
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        let out = parts::grid_kind(
                            ui,
                            cx,
                            &opts,
                            cols,
                            (focus.group == G_SECTIONS).then_some(focus.index),
                            opts.len(),
                            parts::CardKind::Thumb,
                        );
                        groups.push(Group { len: opts.len(), cols, xs: out.xs });
                        if let Some(i) = out.hovered {
                            *hovered = Some(Focus { group: G_SECTIONS, index: i });
                        }
                        if let Some(i) = out.clicked {
                            clicked = Some(Focus { group: G_SECTIONS, index: i });
                        }
                    });
                }
            }
        }
        clicked
    }

    /// The Library / Episodes tabs: sort + view + filters, play row, A–Z bar and the sections.
    fn items_body(
        &mut self,
        ui: &mut egui::Ui,
        cx: &mut Cx,
        groups: &mut Vec<Group>,
        hovered: &mut Option<Focus>,
    ) -> Option<Focus> {
        let t = cx.theme;
        let loaded: Vec<Item> = self.items.iter().filter_map(|o| o.clone()).collect();
        let ordered = self.ordered_loaded();
        self.sort_kinds = self.sort_options(&loaded);
        if !self.dropdown.open {
            self.dropdown.highlighted = self.sort_kinds.iter().position(|&k| k == self.prefs.sort).unwrap_or(0);
        }
        if !self.view_dd.open {
            self.view_dd.highlighted = ViewMode::ALL.iter().position(|&m| m == self.prefs.mode).unwrap_or(0);
        }
        let sort_labels: Vec<String> = self.sort_kinds.iter().map(|k| k.label().to_string()).collect();
        let view_labels: Vec<String> = ViewMode::ALL.iter().map(|m| m.label().to_string()).collect();

        // Sort + View dropdowns (W0), side by side; each opens its menu under itself.
        let mut sort_out = None;
        let mut view_out = None;
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = t.gap;
            ui.vertical(|ui| {
                let focused =
                    self.focus.group == G_DROP && !self.view_dd.open && (self.dropdown.open || self.focus.index == 0);
                sort_out = Some(widgets::dropdown(ui, t, &mut self.dropdown, "Sort", &sort_labels, focused));
            });
            ui.vertical(|ui| {
                let focused =
                    self.focus.group == G_DROP && !self.dropdown.open && (self.view_dd.open || self.focus.index == 1);
                view_out = Some(widgets::dropdown(ui, t, &mut self.view_dd, "View", &view_labels, focused));
            });
        });
        let (Some(sort_out), Some(view_out)) = (sort_out, view_out) else { return None };
        if sort_out.button_hovered && !self.dropdown.open && !self.view_dd.open {
            *hovered = Some(Focus { group: G_DROP, index: 0 });
        }
        if view_out.button_hovered && !self.dropdown.open && !self.view_dd.open {
            *hovered = Some(Focus { group: G_DROP, index: 1 });
        }
        if let Some(i) = sort_out.item_hovered {
            *hovered = Some(Focus { group: G_DROP, index: i });
        }
        if let Some(i) = view_out.item_hovered {
            *hovered = Some(Focus { group: G_DROP, index: i });
        }
        if let Some(i) = sort_out.item_clicked {
            self.dropdown.open = false;
            self.pick_sort(cx, i);
        }
        if let Some(i) = view_out.item_clicked {
            self.view_dd.open = false;
            self.pick_view(cx, i);
        }
        groups[G_DROP] = if self.dropdown.open {
            Group { len: self.sort_kinds.len(), cols: 1, xs: sort_out.item_xs }
        } else if self.view_dd.open {
            Group { len: ViewMode::ALL.len(), cols: 1, xs: view_out.item_xs }
        } else {
            Group { len: 2, cols: 2, xs: vec![sort_out.button_x, view_out.button_x] }
        };

        ui.add_space(t.gap / 3.0);
        let mut open_filters = false;
        let mut clear_filters = false;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = t.gap / 2.0;

            // Ascending/descending toggle (P-LIB-8).
            let tog = widgets::toggle(ui, t, "Descending", self.prefs.descending, self.focus.group == G_TOGGLE);
            if tog.hovered {
                *hovered = Some(Focus { group: G_TOGGLE, index: 0 });
            }
            if tog.clicked {
                self.prefs.descending = !self.prefs.descending;
                self.persist(cx);
                self.reload(cx);
            }
            let mut toggle_xs = vec![tog.x];

            // G11 (P-LIB-10..12): the Filters… dialog, and a way to clear what it set.
            let focused = self.focus == (Focus { group: G_TOGGLE, index: 1 });
            let r = parts::button(ui, t, "Filters…", focused, false);
            toggle_xs.push(r.rect.center().x);
            if r.hovered() {
                *hovered = Some(Focus { group: G_TOGGLE, index: 1 });
            }
            if r.clicked() {
                open_filters = true;
            }
            if !self.prefs.extra.is_empty() {
                let focused = self.focus == (Focus { group: G_TOGGLE, index: 2 });
                let r = parts::button(ui, t, "Clear filters", focused, false);
                toggle_xs.push(r.rect.center().x);
                if r.hovered() {
                    *hovered = Some(Focus { group: G_TOGGLE, index: 2 });
                }
                if r.clicked() {
                    clear_filters = true;
                }
            }
            groups[G_TOGGLE] = Group { len: toggle_xs.len(), cols: toggle_xs.len(), xs: toggle_xs };

            // Filter chip row (P-LIB-9).
            let chips_on = [
                self.prefs.filters.unplayed,
                self.prefs.filters.played,
                self.prefs.filters.resumable,
                self.prefs.filters.favourites,
            ];
            let chip_labels: Vec<String> = CHIP_LABELS.iter().map(|s| s.to_string()).collect();
            let chips_out = widgets::chips(
                ui,
                t,
                &chip_labels,
                &chips_on,
                (self.focus.group == G_CHIPS).then_some(self.focus.index),
            );
            if let Some(i) = chips_out.hovered {
                *hovered = Some(Focus { group: G_CHIPS, index: i });
            }
            if let Some(i) = chips_out.clicked {
                self.toggle_filter(cx, i);
            }
            groups[G_CHIPS] = Group { len: chip_labels.len(), cols: chip_labels.len(), xs: chips_out.xs };
        });
        if open_filters {
            self.open_filters(cx);
        }
        if clear_filters {
            self.prefs.extra = ExtraFilters::default();
            self.persist(cx);
            self.reload(cx);
        }
        if !self.prefs.extra.is_empty() {
            ui.add_space(t.gap / 3.0);
            parts::muted(ui, t, &format!("Filtered by: {}", self.prefs.extra.summary()));
        }

        // Play all / Shuffle (P-LIB-23).
        ui.add_space(t.gap / 3.0);
        let mut play_xs = vec![];
        let mut play_pick: Option<bool> = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = t.gap / 2.0;
            for (i, label) in ["Play all", "Shuffle"].into_iter().enumerate() {
                let focused = self.focus.group == G_PLAY && self.focus.index == i;
                let r = parts::button(ui, t, label, focused, false);
                play_xs.push(r.rect.center().x);
                if r.hovered() {
                    *hovered = Some(Focus { group: G_PLAY, index: i });
                }
                if r.clicked() {
                    play_pick = Some(i == 1);
                }
            }
        });
        groups[G_PLAY] = Group { len: 2, cols: 2, xs: play_xs };
        if let Some(shuffle) = play_pick {
            self.play_library(cx, shuffle);
        }

        ui.add_space(t.gap / 2.0);

        // Loaded items grouped into sections (R-F2-1). Sections grow as later pages land.
        let sections = self.display_sections(&ordered);
        let mode = self.mode();
        let editing = cx.admin && self.prefs.sort == SortBy::MyOrder && mode != ViewMode::List;
        self.az_on = self.prefs.sort == SortBy::Name && self.total > 0;
        let mut pending_jump = self.jump.take();

        let mut clicked: Option<Focus> = None;
        let mut flat_hovered: Option<usize> = None;
        let mut az_clicked: Option<usize> = None;

        // The A–Z bar takes a strip on the right edge; the sections use the rest.
        let outer = ui.available_rect_before_wrap();
        let bar_w = if self.az_on { t.az_w + t.gap / 2.0 } else { 0.0 };
        let main = egui::Rect::from_min_max(outer.min, egui::pos2(outer.right() - bar_w, outer.bottom()));
        ui.scope_builder(egui::UiBuilder::new().max_rect(main), |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                let mut off = 0usize;
                for (si, (label, sec_items)) in sections.iter().enumerate() {
                    let gi = G_SECTIONS + si;
                    let frame = egui::Frame::new()
                        .fill(t.surface)
                        .stroke(egui::Stroke::new(t.section_border_w, t.section_border))
                        .corner_radius(t.section_radius)
                        .inner_margin(t.section_pad);
                    frame.show(ui, |ui| {
                        let (size, kind) = match mode {
                            ViewMode::Thumbs => (egui::vec2(t.thumb_w, t.thumb_h), parts::CardKind::Thumb),
                            ViewMode::Banners => (egui::vec2(t.banner_w, t.banner_h), parts::CardKind::Banner),
                            _ => (egui::vec2(t.poster_w, t.poster_h), parts::CardKind::Poster),
                        };
                        let cols = if mode == ViewMode::List {
                            1
                        } else {
                            nav::grid_cols(ui.available_width(), 0.0, size.x, t.gap)
                        };
                        ui.horizontal(|ui| {
                            parts::heading(ui, t, label);
                            ui.add_space(t.gap / 3.0);
                            parts::muted(ui, t, &format!("{}", sec_items.len()));
                        });
                        ui.add_space(t.gap / 3.0);
                        if mode == ViewMode::List {
                            list_header(ui, t);
                        }
                        let cell = if mode == ViewMode::List {
                            egui::vec2(ui.available_width(), t.list_row_h)
                        } else {
                            egui::vec2(size.x + t.gap, size.y + t.card_label_h + t.gap)
                        };
                        let left = ui.min_rect().left()
                            + if mode == ViewMode::List {
                                0.0
                            } else {
                                ((ui.available_width() - (cols as f32 * cell.x - t.gap)) / 2.0).max(0.0)
                            };
                        let geom = GridGeom { left, top: ui.cursor().top(), cols, cell, size };
                        // P-LIB-13: a jump-bar letter scrolls to the first item starting with it.
                        if let Some(ch) = pending_jump {
                            if let Some(p) = first_with_letter(sec_items, ch) {
                                let r = if mode == ViewMode::List {
                                    egui::Rect::from_min_size(
                                        egui::pos2(geom.left, geom.top + p as f32 * cell.y),
                                        egui::vec2(cell.x, cell.y),
                                    )
                                } else {
                                    card_rect(&geom, t, p)
                                };
                                ui.scroll_to_rect(r, Some(egui::Align::TOP));
                                pending_jump = None;
                            }
                        }
                        let items: Vec<Option<Item>> = sec_items.iter().cloned().map(Some).collect();
                        let focus = (self.focus.group == gi).then_some(self.focus.index);
                        let out = match mode {
                            ViewMode::Posters => parts::grid(ui, cx, &items, cols, focus, sec_items.len()),
                            ViewMode::Thumbs | ViewMode::Banners => {
                                parts::grid_kind(ui, cx, &items, cols, focus, sec_items.len(), kind)
                            }
                            ViewMode::List => list_rows(ui, cx, &items, focus),
                        };
                        groups.push(Group { len: sec_items.len(), cols, xs: out.xs });
                        if let Some(i) = out.hovered {
                            *hovered = Some(Focus { group: gi, index: i });
                            flat_hovered = Some(off + i);
                        }
                        if let Some(i) = out.clicked {
                            clicked = Some(Focus { group: gi, index: i });
                        }
                        // F4 (R-F4-2): the lifted card and drop marker for this section.
                        if mode != ViewMode::List {
                            self.draw_move_visuals(ui, cx, &geom, off, sec_items.len());
                        }
                    });
                    ui.add_space(t.gap);
                    off += sec_items.len();
                }
            });
        });

        // P-LIB-13: the A–Z bar on the right edge. Letters with no loaded item are dimmed.
        if self.az_on {
            let bar = egui::Rect::from_min_max(egui::pos2(outer.right() - t.az_w, outer.top()), outer.max);
            let h = (bar.height() / AZ.len() as f32).min(t.az_h);
            let top = bar.center().y - h * AZ.len() as f32 / 2.0;
            let mut present = [false; 27];
            for it in &ordered {
                if let Some(p) = AZ.iter().position(|&c| c == letter_of(&it.name)) {
                    present[p] = true;
                }
            }
            for (i, &ch) in AZ.iter().enumerate() {
                let r = egui::Rect::from_min_size(egui::pos2(bar.left(), top + i as f32 * h), egui::vec2(t.az_w, h));
                let resp = ui.interact(r, ui.id().with(("g11-az", i)), egui::Sense::click());
                let focused = self.focus.group == G_AZ && self.focus.index == i;
                let ink = if focused || resp.hovered() {
                    t.text
                } else if present[i] {
                    t.muted
                } else {
                    t.muted.gamma_multiply(0.4)
                };
                ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, ch, t.font(t.small_size), ink);
                if focused {
                    parts::focus_ring(ui, t, r);
                }
                if resp.hovered() {
                    *hovered = Some(Focus { group: G_AZ, index: i });
                }
                if resp.clicked() {
                    az_clicked = Some(i);
                }
            }
        }
        if let Some(i) = az_clicked {
            self.focus = Focus { group: G_AZ, index: i };
            self.jump = AZ.get(i).copied();
        }

        // F4 (R-F4-2): mouse drag to reorder (admin + My order). A press that moves past the threshold is a
        // drag; a plain click still opens the item page.
        if editing {
            let pressed = ui.input(|i| i.pointer.any_pressed());
            let released = ui.input(|i| i.pointer.any_released());
            let down = ui.input(|i| i.pointer.primary_down());
            let ptr = ui.input(|i| i.pointer.hover_pos());
            if pressed {
                if let Some(h) = flat_hovered {
                    self.drag = Some(DragState { from: h, to: h, ptr: ptr.unwrap_or_default(), moved: false });
                }
            }
            if let Some(mut d) = self.drag {
                if down {
                    if let Some(pos) = ptr {
                        if (pos - d.ptr).length() > t.gap / 2.0 {
                            d.moved = true;
                        }
                        d.ptr = pos;
                    }
                    if let Some(h) = flat_hovered {
                        if h != d.to {
                            d.moved = true;
                            d.to = h;
                        }
                    }
                    self.drag = Some(d);
                }
                if released {
                    if d.moved {
                        let (from, to) = (d.from, d.to);
                        self.drop_move(cx, from, to);
                        clicked = None;
                    }
                    self.drag = None;
                }
            }
        }
        clicked
    }
}

impl Screen for Library {
    fn name(&self) -> &'static str {
        "library"
    }

    fn on_key(&mut self, cx: &mut Cx, key: Key) -> bool {
        if self.dlg.is_some() {
            return self.dlg_key(cx, key);
        }
        if self.tab.is_items() && self.error.is_some() {
            return match key {
                Key::Enter => {
                    self.reload(cx);
                    true
                }
                _ => false,
            };
        }
        if let Some(mut m) = self.moving {
            return match key {
                Key::Left | Key::Up => {
                    m.to = m.to.saturating_sub(1);
                    self.moving = Some(m);
                    true
                }
                Key::Right | Key::Down => {
                    let len = self.ordered_loaded().len();
                    m.to = (m.to + 1).min(len.saturating_sub(1));
                    self.moving = Some(m);
                    true
                }
                Key::Enter => {
                    self.moving = None;
                    self.drop_move(cx, m.from, m.to);
                    true
                }
                Key::Back => {
                    self.moving = None;
                    true
                }
                _ => false,
            };
        }
        if self.dropdown.open {
            let (state, ev) = widgets::dropdown_key(self.dropdown, key, self.sort_kinds.len());
            self.dropdown = state;
            if let Some(widgets::DropdownEvent::Pick(i)) = ev {
                self.pick_sort(cx, i);
            }
            return matches!(key, Key::Up | Key::Down | Key::Enter | Key::Back);
        }
        if self.view_dd.open {
            let (state, ev) = widgets::dropdown_key(self.view_dd, key, ViewMode::ALL.len());
            self.view_dd = state;
            if let Some(widgets::DropdownEvent::Pick(i)) = ev {
                self.pick_view(cx, i);
                self.focus = Focus { group: G_DROP, index: 1 };
            }
            return matches!(key, Key::Up | Key::Down | Key::Enter | Key::Back);
        }
        // F4 (R-F4-2): Info on a card enters move mode (admin, My order sort).
        if key == Key::Info && cx.admin && self.prefs.sort == SortBy::MyOrder && self.tab.is_items() {
            if let Some(flat) = self.focused_flat_index() {
                self.moving = Some(MoveState { from: flat, to: flat });
                return true;
            }
        }
        // P-LIB-13: the A–Z bar is vertical, so Up/Down walk the letters and Left returns to the grid.
        if self.focus.group == G_AZ && self.az_on {
            match key {
                Key::Up if self.focus.index > 0 => {
                    self.focus.index -= 1;
                    return true;
                }
                Key::Up => {
                    self.focus = Focus { group: G_PLAY, index: 0 };
                    return true;
                }
                Key::Down => {
                    self.focus.index = (self.focus.index + 1).min(AZ.len() - 1);
                    return true;
                }
                Key::Left => {
                    self.focus = Focus { group: G_SECTIONS, index: 0 };
                    return true;
                }
                Key::Right => return true,
                Key::Enter => {
                    self.jump = AZ.get(self.focus.index).copied();
                    return true;
                }
                _ => {}
            }
        }
        match key {
            Key::Enter => match self.focus.group {
                G_TABS => {
                    if let Some(&tb) = self.tabs.get(self.focus.index) {
                        self.set_tab(cx, tb);
                    }
                    true
                }
                G_DROP => {
                    if self.focus.index == 1 {
                        let (state, _) = widgets::dropdown_key(self.view_dd, key, ViewMode::ALL.len());
                        self.view_dd = state;
                    } else {
                        let (state, _) = widgets::dropdown_key(self.dropdown, key, self.sort_kinds.len());
                        self.dropdown = state;
                    }
                    true
                }
                G_TOGGLE if self.focus.index == 1 => {
                    self.open_filters(cx);
                    true
                }
                G_TOGGLE if self.focus.index == 2 => {
                    self.prefs.extra = ExtraFilters::default();
                    self.persist(cx);
                    self.reload(cx);
                    true
                }
                G_PLAY => {
                    self.play_library(cx, self.focus.index == 1);
                    true
                }
                G_TOGGLE => {
                    self.prefs.descending = !self.prefs.descending;
                    self.persist(cx);
                    self.reload(cx);
                    true
                }
                G_CHIPS => {
                    let (f, ev) = widgets::chips_key(self.focus.index, CHIP_LABELS.len(), Key::Enter);
                    self.focus.index = f;
                    if let Some(i) = ev {
                        self.toggle_filter(cx, i);
                    }
                    true
                }
                _ => {
                    self.activate(cx, self.focus);
                    true
                }
            },
            Key::Back => {
                if self.dropdown.open {
                    self.dropdown.open = false;
                    true
                } else {
                    false
                }
            }
            Key::Left | Key::Right => {
                if self.focus.group == G_CHIPS {
                    let (f, _) = widgets::chips_key(self.focus.index, CHIP_LABELS.len(), key);
                    self.focus.index = f;
                } else {
                    let next = nav::move_focus(&self.groups, self.focus, key);
                    // Right past the last column of the grid steps onto the A–Z bar.
                    if key == Key::Right && next == self.focus && self.focus.group >= G_SECTIONS && self.az_on {
                        let at = self.item_at(self.focus).map(|i| letter_of(&i.name));
                        let index = at.and_then(|c| AZ.iter().position(|&a| a == c)).unwrap_or(0);
                        self.focus = Focus { group: G_AZ, index };
                    } else {
                        self.focus = next;
                    }
                }
                true
            }
            Key::Up | Key::Down => {
                self.focus = nav::move_focus(&self.groups, self.focus, key);
                true
            }
            _ => false,
        }
    }

    fn on_done(&mut self, cx: &mut Cx, tag: &'static str, payload: Box<dyn std::any::Any + Send>) {
        if tag == "playall" {
            self.play_loading = false;
            let Ok(r) = payload.downcast::<Result<Vec<Item>>>() else { return };
            match *r {
                Ok(items) if !items.is_empty() => cx.push(super::player::screen_queue(
                    items,
                    super::player::PlayOptions { from_start: true, ..Default::default() },
                )),
                Ok(_) => {}
                Err(ApiError::Unauthorized) => cx.signed_out(),
                Err(e) => self.error = Some(e.to_string()),
            }
            return;
        }
        if tag == "order" {
            let Ok(r) = payload.downcast::<Result<()>>() else { return };
            match *r {
                Ok(()) => {}
                Err(ApiError::Unauthorized) => cx.signed_out(),
                Err(e) => {
                    self.move_error = Some(e.to_string());
                    self.reload(cx);
                }
            }
            return;
        }
        if tag == "fopts" {
            let Ok(r) = payload.downcast::<Result<FilterOptions>>() else { return };
            match *r {
                Ok(o) => self.fopts = Some(o),
                Err(ApiError::Unauthorized) => cx.signed_out(),
                // The dialog still works without the lists (years and features need none).
                Err(_) => self.fopts = Some(FilterOptions::default()),
            }
            return;
        }
        if tag == "tab" {
            let Ok(p) = payload.downcast::<(u64, Result<Side>)>() else { return };
            let (gen, result) = *p;
            if gen != self.side_gen || self.tab.is_items() {
                return;
            }
            match result {
                Err(ApiError::Unauthorized) => cx.signed_out(),
                r => self.side = Some(r),
            }
            return;
        }
        if tag != "page" {
            return;
        }
        let Ok(p) = payload.downcast::<(u64, Result<Page>)>() else { return };
        let (gen, result) = *p;
        if gen != self.gen {
            return;
        }
        self.in_flight = false;
        match result {
            Ok(page) => {
                self.total = page.total_record_count;
                if self.items.len() < self.total {
                    self.items.resize(self.total, None);
                }
                let start = self.next_start;
                let n = page.items.len();
                for (i, item) in page.items.into_iter().enumerate() {
                    if start + i < self.items.len() {
                        self.items[start + i] = Some(item);
                    }
                }
                self.next_start = if n == 0 { self.total } else { start + n };
            }
            Err(ApiError::Unauthorized) => cx.signed_out(),
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        if !self.started {
            self.started = true;
            self.prefs = cx.settings.library_view.get(&self.view.id).cloned().unwrap_or_default();
            self.request_page(cx);
        }
        let t = cx.theme;
        let pointer_moved = ui.input(|i| i.pointer.is_moving());

        ui.add_space(t.margin);
        parts::title(ui, t, &self.view.name);
        if self.tab.is_items() {
            parts::muted(ui, t, &format!("{} items", self.total));
        }
        ui.add_space(t.gap / 2.0);

        let mut groups: Vec<Group> = vec![Group::default(); G_SECTIONS];
        let mut hovered: Option<Focus> = None;

        // G11 (P-LIB-16, 18..21): tabs above the grid.
        if self.tabs.len() > 1 {
            let labels: Vec<String> = self.tabs.iter().map(|x| x.label().to_string()).collect();
            let sel = self.tabs.iter().position(|&x| x == self.tab).unwrap_or(0);
            let out = widgets::tabs(ui, t, &labels, sel, (self.focus.group == G_TABS).then_some(self.focus.index));
            if let Some(i) = out.hovered {
                hovered = Some(Focus { group: G_TABS, index: i });
            }
            groups[G_TABS] = Group { len: labels.len(), cols: labels.len(), xs: out.xs };
            if let Some(&tb) = out.clicked.and_then(|i| self.tabs.get(i)) {
                self.set_tab(cx, tb);
            }
            ui.add_space(t.gap / 2.0);
        }

        if self.tab.is_items() {
            if let Some(e) = self.error.clone() {
                parts::error_line(ui, t, &e);
                let r = parts::button(ui, t, "Retry", false, false);
                if r.clicked() {
                    self.reload(cx);
                }
                self.groups = groups;
                return;
            }
        }

        let clicked = if self.tab.is_items() {
            self.items_body(ui, cx, &mut groups, &mut hovered)
        } else {
            self.side_body(ui, cx, &mut groups, &mut hovered)
        };

        self.groups = groups;
        // The A–Z bar is not a nav group (it is vertical); keep a focus on it as it is.
        if !(self.focus.group == G_AZ && self.az_on) {
            self.focus = nav::move_focus(&self.groups, self.focus, Key::Enter);
        }
        if pointer_moved && self.dlg.is_none() {
            if let Some(f) = hovered {
                self.focus = f;
            }
        }
        if self.dlg.is_none() {
            if let Some(f) = clicked {
                self.activate(cx, f);
            }
        }

        if self.moving.is_some() {
            ui.add_space(t.gap / 2.0);
            parts::muted(ui, t, "Move: arrows · Enter to drop · Back to cancel");
        }
        if let Some(e) = self.move_error.clone() {
            ui.add_space(t.gap / 2.0);
            parts::error_line(ui, t, &e);
        }
        if let Some(from) = self.drag.map(|d| d.from) {
            if let Some(item) = self.flat_item(from) {
                let ptr = self.drag.map(|d| d.ptr).unwrap_or_default();
                parts::drag_ghost(ui, cx, &item, parts::CardKind::Poster, ptr);
            }
        }

        // Load every page in the background; the first page is already drawn.
        if self.tab.is_items() && !self.in_flight && self.next_start < self.total {
            self.request_page(cx);
        }

        self.dialog_ui(ui, cx, pointer_moved);
    }
}

// ---------- helpers ----------

fn toggle(v: &mut Vec<String>, s: &str) {
    if let Some(p) = v.iter().position(|x| x == s) {
        v.remove(p);
    } else {
        v.push(s.to_string());
    }
}

fn feature_on(x: &ExtraFilters, i: usize) -> bool {
    let mut c = x.clone();
    feature_flag(&mut c, i).map(|f| *f).unwrap_or(false)
}

/// A digit typed into a year field: appends while the number is short, otherwise starts again.
fn year_type(v: Option<u32>, c: char) -> Option<u32> {
    let d = c.to_digit(10)?;
    Some(match v {
        Some(x) if x < 1000 => x * 10 + d,
        _ => d,
    })
}

fn year_back(v: Option<u32>) -> Option<u32> {
    v.and_then(|x| if x < 10 { None } else { Some(x / 10) })
}

fn dlg_section(it: &DlgItem) -> Option<&'static str> {
    match it {
        DlgItem::Genre(_) => Some("Genres"),
        DlgItem::Rating(_) => Some("Parental rating"),
        DlgItem::Tag(_) => Some("Tags"),
        DlgItem::YearFrom | DlgItem::YearTo => Some("Years"),
        DlgItem::Feature(i) if *i < 5 => Some("Features"),
        DlgItem::Feature(_) => Some("Video"),
        DlgItem::Clear | DlgItem::Apply => None,
    }
}

/// A toggle chip that wraps with its neighbours (W0's `chips` is a single row).
fn dlg_chip(ui: &mut egui::Ui, t: &Theme, label: &str, on: bool, focused: bool) -> egui::Response {
    let galley = ui.fonts(|f| f.layout_no_wrap(label.to_string(), t.font(t.body_size), t.text));
    let size = egui::vec2(galley.size().x + 2.0 * t.chip_pad, t.chip_h);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    let (fill, ink) = if on { (t.accent, t.accent_text) } else { (t.surface_hi, t.text) };
    ui.painter().rect_filled(rect, t.chip_h / 2.0, fill);
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, label, t.font(t.body_size), ink);
    if focused {
        parts::focus_ring(ui, t, rect);
    }
    resp
}

/// A year box: the typed year, or "Any".
fn year_field(ui: &mut egui::Ui, t: &Theme, v: Option<u32>, focused: bool) -> egui::Response {
    let text = v.filter(|&y| y > 0).map(|y| y.to_string()).unwrap_or_else(|| "Any".to_string());
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(t.field_w / 3.0, t.chip_h), egui::Sense::click());
    ui.painter().rect_filled(rect, t.radius, t.surface_hi);
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, text, t.font(t.body_size), t.text);
    if focused {
        parts::focus_ring(ui, t, rect);
    }
    resp
}

/// P-LIB-14: the column titles above the list view's rows.
fn list_header(ui: &mut egui::Ui, t: &Theme) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), t.small_size + t.gap / 2.0), egui::Sense::hover());
    for (x, label) in list_cols(rect.width()) {
        ui.painter().text(
            egui::pos2(rect.left() + x, rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            t.font(t.small_size),
            t.muted,
        );
    }
}

/// x offsets of the list view's columns for a row `w` wide.
fn list_cols(w: f32) -> [(f32, &'static str); 4] {
    [(16.0, "Title"), (w * 0.55, "Year"), (w * 0.65, "Runtime"), (w * 0.80, "Rating")]
}

/// P-LIB-14: the list view — one row per item: title, year, runtime, rating. Only visible rows are drawn.
fn list_rows(ui: &mut egui::Ui, cx: &mut Cx, items: &[Option<Item>], focus: Option<usize>) -> parts::RowOut {
    let t = cx.theme;
    let mut out = parts::RowOut { clicked: None, hovered: None, xs: vec![] };
    let w = ui.available_width();
    let top = ui.cursor().top();
    let left = ui.min_rect().left();
    ui.allocate_exact_size(egui::vec2(w, items.len() as f32 * t.list_row_h), egui::Sense::hover());
    let clip = ui.clip_rect();
    let cols = list_cols(w);
    for (i, slot) in items.iter().enumerate() {
        let rect = egui::Rect::from_min_size(
            egui::pos2(left, top + i as f32 * t.list_row_h),
            egui::vec2(w, t.list_row_h - t.gap / 4.0),
        );
        out.xs.push(rect.center().x);
        if focus == Some(i) {
            ui.scroll_to_rect(rect.expand(t.gap), None);
        }
        if !rect.intersects(clip) {
            continue;
        }
        let resp = ui.interact(rect, ui.id().with(("g11-row", i)), egui::Sense::click());
        let fill = if resp.hovered() || focus == Some(i) { t.surface_hi } else { t.surface };
        ui.painter().rect_filled(rect, t.radius, fill);
        let Some(it) = slot else { continue };
        let runtime = it.run_time_ticks.map(|x| parts::runtime(crate::api::ticks_to_secs(x))).unwrap_or_default();
        let year = it.production_year.map(|y| y.to_string()).unwrap_or_default();
        let rating = match (it.community_rating, &it.official_rating) {
            (Some(c), Some(o)) => format!("{c:.1} · {o}"),
            (Some(c), None) => format!("{c:.1}"),
            (None, Some(o)) => o.clone(),
            (None, None) => String::new(),
        };
        let title_w = cols[1].0 - cols[0].0 - t.gap;
        parts::label_line(
            ui,
            t,
            egui::pos2(rect.left() + cols[0].0, rect.center().y - t.body_size / 2.0),
            title_w,
            &it.name,
            t.body_size,
            t.text,
        );
        for (k, text) in [(1usize, year), (2, runtime), (3, rating)] {
            ui.painter().text(
                egui::pos2(rect.left() + cols[k].0, rect.center().y),
                egui::Align2::LEFT_CENTER,
                text,
                t.font(t.body_size),
                t.muted,
            );
        }
        if focus == Some(i) {
            parts::focus_ring(ui, t, rect);
        }
        if resp.hovered() {
            out.hovered = Some(i);
        }
        if resp.clicked() {
            out.clicked = Some(i);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_drop_articles_and_bucket_the_rest() {
        assert_eq!(letter_of("The Matrix"), 'M');
        assert_eq!(letter_of("A Quiet Place"), 'Q');
        assert_eq!(letter_of("An Elephant"), 'E');
        assert_eq!(letter_of("Archive"), 'A');
        assert_eq!(letter_of("3 Men"), '#');
        assert_eq!(letter_of("  zed"), 'Z');
        assert_eq!(letter_of(""), '#');
        assert_eq!(letter_of("The"), 'T');
    }

    #[test]
    fn jump_finds_the_first_item_for_a_letter() {
        let it = |n: &str| Item { name: n.to_string(), ..Default::default() };
        let items = [it("Amber Tide"), it("The Beacon"), it("Blue Harbour"), it("12 Years")];
        assert_eq!(first_with_letter(&items, 'A'), Some(0));
        assert_eq!(first_with_letter(&items, 'B'), Some(1));
        assert_eq!(first_with_letter(&items, '#'), Some(3));
        assert_eq!(first_with_letter(&items, 'Z'), None);
    }

    #[test]
    fn tabs_depend_on_library_kind() {
        assert_eq!(tabs_for(Some("movies")), vec![Tab::Library, Tab::Suggestions, Tab::Genres, Tab::Collections]);
        assert_eq!(
            tabs_for(Some("tvshows")),
            vec![Tab::Library, Tab::Genres, Tab::Studios, Tab::Upcoming, Tab::Episodes]
        );
        assert_eq!(tabs_for(None), vec![Tab::Library, Tab::Genres]);
    }

    #[test]
    fn year_typing() {
        assert_eq!(year_type(None, '1'), Some(1));
        assert_eq!(year_type(Some(199), '9'), Some(1999));
        assert_eq!(year_type(Some(1999), '2'), Some(2));
        assert_eq!(year_back(Some(1999)), Some(199));
        assert_eq!(year_back(Some(5)), None);
    }

    #[test]
    fn dialog_movement_follows_screen_position() {
        let r = |x: f32, y: f32| egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(50.0, 20.0));
        // two rows of chips, then a footer button
        let rects = [r(0.0, 0.0), r(60.0, 0.0), r(120.0, 0.0), r(0.0, 40.0), r(60.0, 40.0), r(100.0, 80.0)];
        assert_eq!(dlg_move(&rects, 1, Key::Down), 4);
        assert_eq!(dlg_move(&rects, 2, Key::Down), 4);
        assert_eq!(dlg_move(&rects, 4, Key::Up), 1);
        assert_eq!(dlg_move(&rects, 4, Key::Down), 5);
        assert_eq!(dlg_move(&rects, 0, Key::Up), 0);
        assert_eq!(dlg_move(&rects, 5, Key::Down), 5);
        assert_eq!(dlg_move(&rects, 0, Key::Left), 0);
        assert_eq!(dlg_move(&rects, 0, Key::Right), 1);
    }
}
