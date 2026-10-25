//! The app: a stack of screens driven one frame at a time by the shell (R-PLAT-4, R-STATE-*).
use crate::api::poster::all_poster_prefs;
use crate::api::{ApiError, Client, DeviceInfo};
use crate::images::DiskCache;
use crate::platform::{Key, Player, Storage};
use crate::settings::Settings;
use crate::tasks::Pool;
use crate::theme::Theme;
use crate::ui::{self, imageload::Images};
use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;

/// One screen. Screens own their data and focus; they talk to the rest of the app through `Cx`.
pub trait Screen {
    /// Short stable name ("home", "library", …) used by scripts and logs.
    fn name(&self) -> &'static str;
    /// Arrow keys, Enter, Back … for this frame (Back is handled here; return true if consumed,
    /// otherwise the app pops the screen).
    fn on_key(&mut self, cx: &mut Cx, key: Key) -> bool {
        let _ = (cx, key);
        false
    }
    /// A result of `cx.spawn(tag, …)` for this screen.
    fn on_done(&mut self, cx: &mut Cx, tag: &'static str, payload: Box<dyn Any + Send>) {
        let _ = (cx, tag, payload);
    }
    /// Draw. Called every frame after keys and results.
    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx);
    /// Player screens: the shell draws video under the UI.
    fn shows_video(&self) -> bool {
        false
    }
    /// Called when the screen comes back to the top (e.g. refresh resume state after playback).
    fn on_resume(&mut self, cx: &mut Cx) {
        let _ = cx;
    }
    /// PP: `(item id, Primary ImageTag)` of every poster this screen shows (tests: did a refresh pick up a new tag?).
    fn poster_tags(&self) -> Vec<(String, Option<String>)> {
        vec![]
    }
    /// Called when popped or the app quits (player: report stopped).
    fn on_leave(&mut self, cx: &mut Cx) {
        let _ = cx;
    }
    /// NAV: this screen's part of the toolbar breadcrumb ("Drift Rebels", "Season 2"). Empty → a label from
    /// `name()` (see `ui::shell::crumbs_of`).
    fn crumbs(&self) -> Vec<String> {
        vec![]
    }
}

pub enum Nav {
    Push(Box<dyn Screen>),
    Pop,
    /// Clear the stack and start from this screen (sign-in done, sign out).
    Reset(Box<dyn Screen>),
    /// NAV: re-open the screen the last Back left.
    Forward,
    /// NAV: keep the first `n` screens; the ones above go to the forward history (breadcrumb click).
    PopTo(usize),
    /// NAV: a sidebar place — back to the first screen (Home), then open this one on top (None = Home itself).
    Section(Option<Box<dyn Screen>>),
}

/// What a screen can reach during a frame.
pub struct Cx<'a> {
    pub theme: &'a Theme,
    pub client: &'a mut Option<Client>,
    pub settings: &'a mut Settings,
    pub store: &'a Arc<dyn Storage>,
    pub tasks: &'a Pool,
    pub images: &'a mut Images,
    pub player: &'a mut dyn Player,
    pub opts: &'a AppOptions,
    /// R-TAG-6: true once the signed-in user is known to be a server admin (false until known).
    pub admin: bool,
    /// PP-4: the signed-in user's personal poster picks (item id → poster URL), applied this session.
    pub poster_overrides: &'a mut std::collections::HashMap<String, String>,
    /// PP-4: item ids whose shared poster changed this session; library grids re-read them on resume.
    pub poster_changed: &'a mut Vec<String>,
    /// PP review: counts shared-poster changes (admin apply/undo); a screen that remembers the value it last
    /// loaded at re-reads its items on resume when this moved on.
    pub poster_gen: u64,
    /// PP-5: the pending poster Undo (app-level, so it survives leaving the picker).
    pub undo: &'a mut Option<PosterUndo>,
    /// R-PLAT-3: keys physically held down this frame (the TV hold-OK uses Enter's key-down state).
    pub keys_down: &'a std::collections::HashSet<Key>,
    /// Requests to the window (fullscreen…), read by the shell after the frame.
    pub shell: &'a mut crate::platform::ShellRequests,
    /// Seconds since the app started (for timers: debounce, OSD hide, countdowns).
    pub now: f64,
    /// NAV: the desktop shell draws the sidebar + toolbar around this screen (its own top bar is not needed).
    pub sidebar: bool,
    pub(crate) screen_id: u64,
    pub(crate) nav: &'a mut Vec<Nav>,
}

impl Cx<'_> {
    pub fn push(&mut self, s: Box<dyn Screen>) {
        self.nav.push(Nav::Push(s));
    }
    pub fn pop(&mut self) {
        self.nav.push(Nav::Pop);
    }
    pub fn reset(&mut self, s: Box<dyn Screen>) {
        self.nav.push(Nav::Reset(s));
    }
    /// NAV: re-open the screen the last Back left (no-op without one).
    pub fn forward(&mut self) {
        self.nav.push(Nav::Forward);
    }
    /// NAV: go to a top-level place: back to Home, then `s` on top (None = Home).
    pub fn section(&mut self, s: Option<Box<dyn Screen>>) {
        self.nav.push(Nav::Section(s));
    }
    /// Blocking work on the pool; the result comes back to this screen's `on_done` as `Box<T>`.
    pub fn spawn<T: Send + 'static>(&self, tag: &'static str, f: impl FnOnce() -> T + Send + 'static) {
        self.tasks.spawn(self.screen_id, tag, f);
    }
    /// Signed-in client (screens past sign-in can rely on it).
    pub fn api(&self) -> Client {
        self.client.clone().expect("signed in")
    }
    /// PP-4: the signed-in user's personal poster override for `item`, if they picked one this session.
    pub fn poster_url(&self, item: &crate::api::Item) -> Option<String> {
        self.poster_overrides.get(&item.id).filter(|u| self.images.usable(u)).cloned()
    }
    /// PP-4: set (`Some`) or clear (`None`) a personal poster override, and persist it so the pick survives a
    /// restart (loaded at sign-in).
    pub fn set_poster_override(&mut self, item_id: &str, url: Option<String>) {
        match url {
            Some(u) => {
                self.poster_overrides.insert(item_id.to_string(), u);
            }
            None => {
                self.poster_overrides.remove(item_id);
            }
        }
        if let Some(uid) = self.client.as_ref().and_then(|c| c.session.as_ref()).map(|s| s.user_id.clone()) {
            self.settings.poster_picks.insert(uid, self.poster_overrides.clone());
        }
        self.save_settings();
    }
    /// FIX-4: the signed-in viewer's remembered subtitle for `item`; None unless their Jellyfin "Remember subtitle
    /// selections" is on.
    pub fn remembered_subtitle(&self, item: &crate::api::Item) -> Option<crate::settings::SubPick> {
        let uid = self.client.as_ref().and_then(|c| c.session.as_ref()).map(|s| s.user_id.as_str())?;
        self.settings.remembered_sub(uid, item).cloned()
    }
    /// FIX-4: keep the viewer's subtitle pick `val` (None = Off) of `ms`'s streams for `item`, saved with the
    /// settings, when their "Remember subtitle selections" is on.
    pub fn remember_subtitle(
        &mut self,
        item: &crate::api::Item,
        ms: Option<&crate::api::MediaSource>,
        val: Option<i32>,
    ) {
        let Some(uid) = self.client.as_ref().and_then(|c| c.session.as_ref()).map(|s| s.user_id.clone()) else {
            return;
        };
        let stream = val.and_then(|i| {
            ms?.media_streams.iter().find(|s| s.kind == crate::api::StreamKind::Subtitle && s.index == i)
        });
        let pick = crate::settings::SubPick {
            index: val.unwrap_or(-1),
            language: stream.and_then(|s| s.language.clone()),
            forced: stream.is_some_and(|s| s.is_forced),
        };
        if self.settings.remember_sub(&uid, item, pick) {
            self.save_settings();
        }
    }
    pub fn device(&self) -> DeviceInfo {
        device_info(self.settings, self.opts)
    }
    pub fn save_settings(&self) {
        let _ = self.settings.save(self.store.as_ref());
    }
    /// R-API-5: any screen that gets `Unauthorized` calls this → back to sign-in for the same server.
    pub fn signed_out(&mut self) {
        if let Some(c) = self.client.as_mut() {
            c.session = None;
        }
        if let Some(id) = self.settings.last_server.clone() {
            self.settings.forget_session(&id);
            self.save_settings();
        }
        let c = self.client.clone();
        if let Some(c) = c {
            self.reset(ui::signin::screen(c, Some(ApiError::Unauthorized.to_string())));
        } else {
            self.reset(ui::connect::screen(None));
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct AppOptions {
    /// Pre-filled server address on the Connect screen (R-SET-3).
    pub default_server: Option<String>,
    /// Shown to the server as Device="…" (R-API-1).
    pub device_name: String,
    /// A screen to open on top after the sign-in state is decided (`--screen`), e.g. "gallery".
    pub start_screen: Option<String>,
    /// NAV: desktop shell — sidebar + Back/Forward toolbar around viewer screens (the TV build leaves it off).
    pub desktop: bool,
    /// PP (test-only): `--poster-candidates <json>` replaces the RemoteImages reply with this fixture so the
    /// picker can be shown and shot without internet (image URLs are `file://` PNGs).
    pub poster_candidates: Option<String>,
    /// PP (test-only): the folder of the `--poster-candidates` fixture file; its `file://` images may be loaded.
    pub poster_fixture_dir: Option<std::path::PathBuf>,
}

fn device_info(s: &Settings, o: &AppOptions) -> DeviceInfo {
    DeviceInfo {
        name: if o.device_name.is_empty() { "Desktop".into() } else { o.device_name.clone() },
        id: s.device_id.clone(),
        version: env!("CARGO_PKG_VERSION").into(),
    }
}

/// Screen id reserved for the admin check result (real screen ids start at 1).
const ADMIN_SCREEN: u64 = 0;
const ADMIN_TAG: &str = "admin";
/// FIX-4: the signed-in user's "Remember subtitle selections" (read with the admin check).
const REMEMBER_SUBS_TAG: &str = "remember-subs";
/// NAV: the server's libraries for the sidebar (loaded with the admin check).
const VIEWS_TAG: &str = "views";
/// Screen id reserved for the screensaver's backdrop list.
const SAVER_SCREEN: u64 = u64::MAX;
const SAVER_TAG: &str = "saver";
/// Screen id reserved for the poster-undo result (PP-5).
const UNDO_SCREEN: u64 = u64::MAX - 1;
/// Screen id reserved for the user's personal poster picks read from the server.
const PICKS_SCREEN: u64 = u64::MAX - 2;
const PICKS_TAG: &str = "poster-picks";
const UNDO_TAG: &str = "poster-undo";

/// PP-5: what an Undo needs, held at the app level so the toast survives leaving the picker.
pub struct PosterUndo {
    pub item_id: String,
    pub admin: bool,
    pub previous: Option<Vec<u8>>,
    pub display_id: Option<String>,
    pub at: f64,
}

pub struct App {
    pub theme: Theme,
    pub opts: AppOptions,
    client: Option<Client>,
    settings: Settings,
    store: Arc<dyn Storage>,
    tasks: Pool,
    pub images: Images,
    stack: Vec<(u64, Box<dyn Screen>)>,
    /// NAV: screens Back left, nearest last; Forward re-opens them. Opening a new screen clears it.
    forward: Vec<(u64, Box<dyn Screen>)>,
    /// NAV: the signed-in user's libraries (sidebar).
    views: Vec<crate::api::Item>,
    /// NAV: the keyboard was used last (focus marks show).
    keyboard: bool,
    /// NAV: keyboard focus in the sidebar (index into `shell::entries`); None = in the screen.
    side: Option<usize>,
    /// UX D30: the sidebar's "Sign out?" confirmation, the focused button (0 = Cancel).
    confirm: Option<usize>,
    /// NAV: this frame's keys come from a held (auto-repeating) key — they never leave the screen for the sidebar.
    pub keys_held: bool,
    next_id: u64,
    started: crate::time::Instant,
    /// Recording (the `record` script command): each frame moves the clock on by this step, not by wall time.
    fixed_step: Option<f64>,
    fixed_now: f64,
    themed: bool,
    client_key: Option<(String, Option<String>)>,
    admin: bool,
    shell: crate::platform::ShellRequests,
    /// RR8 / P-USER-2: app time of the last key or pointer input, and the screensaver while it is up.
    last_input: f64,
    saver: Option<ui::screensaver::Overlay>,
    /// RR8 / P-USER-1: files the shell's picker returned, handed to the top screen next frame.
    picked: Vec<String>,
    /// PP-4: the signed-in user's personal poster picks (item id → poster URL), applied this session.
    poster_overrides: std::collections::HashMap<String, String>,
    /// PP-4: item ids whose shared poster changed; library grids re-read them on resume.
    poster_changed: Vec<String>,
    poster_gen: u64,
    /// An admin Undo finished: re-read the visible screen as soon as this frame's results are in.
    refresh_top: bool,
    /// PP-5: the pending poster Undo (app-level toast).
    undo: Option<PosterUndo>,
    /// R-PLAT-3: keys physically held down this frame (set by the shell).
    pub keys_down: std::collections::HashSet<Key>,
    /// Added to the clock (render scripts fast-forward the idle time with it).
    skew: f64,
    /// UXP5 (TV build): the on-screen keyboard while it is open, the text field it types into (as drawn last
    /// frame), and the screen it last opened by itself on (Search opens it on arrival, once).
    tv_kb: Option<ui::keyboard::KeyboardState>,
    tv_field: Option<ui::keyboard::TvField>,
    tv_kb_auto: u64,
    /// FIX-5: the keyboard as it was closed, per field of the current screen — screen id, field hint, password
    /// flag, state — so Enter on the same field opens it on the key the viewer left.
    tv_kb_saved: Vec<(u64, String, bool, ui::keyboard::KeyboardState)>,
    /// FIX-1 #10: when text last arrived from a physical keyboard (TV); typing stands the on-screen keyboard
    /// down only for `TV_TEXT_QUIET` seconds after it, so the remote keyboard never latches off.
    tv_text_at: Option<f64>,
}

impl App {
    /// R-SET-2: a saved session opens Home (checked in the background); otherwise Connect.
    pub fn new(store: Arc<dyn Storage>, opts: AppOptions) -> App {
        let settings = Settings::load(store.as_ref());
        if store.read(crate::settings::SETTINGS_KEY).is_none() {
            let _ = settings.save(store.as_ref());
        }
        let disk = DiskCache::new(store.cache_dir().join("img"), 500 << 20).ok();
        let mut app = App {
            theme: Theme::for_choice(settings.theme),
            opts,
            client: None,
            settings,
            store,
            tasks: Pool::new(4),
            images: Images::new(disk),
            stack: vec![],
            forward: vec![],
            views: vec![],
            keyboard: true,
            side: None,
            confirm: None,
            keys_held: false,
            next_id: 1,
            started: crate::time::Instant::now(),
            fixed_step: None,
            fixed_now: 0.0,
            themed: false,
            client_key: None,
            admin: false,
            shell: Default::default(),
            last_input: 0.0,
            saver: None,
            picked: vec![],
            poster_overrides: std::collections::HashMap::new(),
            poster_changed: vec![],
            poster_gen: 0,
            refresh_top: false,
            undo: None,
            keys_down: std::collections::HashSet::new(),
            skew: 0.0,
            tv_kb: None,
            tv_field: None,
            tv_kb_auto: 0,
            tv_kb_saved: Vec::new(),
            tv_text_at: None,
        };
        let mut roots = vec![crate::api::poster::poster_store_dir(&app.store.cache_dir())];
        roots.extend(app.opts.poster_fixture_dir.clone());
        app.images.set_file_roots(roots);
        let first: Box<dyn Screen> = match app.settings.last().cloned() {
            Some(e) if e.session.is_some() => {
                let c = Client::with_session(&e.address, app.device(), e.session.unwrap());
                app.client = Some(c);
                ui::home::screen(true)
            }
            Some(e) => {
                let c = Client::new(&e.address, app.device());
                app.client = Some(c.clone());
                ui::signin::screen(c, None)
            }
            None => ui::connect::screen(app.opts.default_server.clone()),
        };
        app.push(first);
        if app.opts.start_screen.as_deref() == Some("gallery") {
            app.push(ui::gallery::screen());
        }
        app
    }

    pub fn device(&self) -> DeviceInfo {
        device_info(&self.settings, &self.opts)
    }

    fn push(&mut self, s: Box<dyn Screen>) {
        self.forward.clear();
        self.stack.push((self.next_id, s));
        self.next_id += 1;
    }

    /// Tests: put a screen on top (as a click would).
    pub fn open_screen(&mut self, s: Box<dyn Screen>) {
        self.push(s);
    }
    /// Tests: the shared poster of `item_id` changed (what the picker reports after an admin apply).
    pub fn mark_poster_changed(&mut self, item_id: &str) {
        self.poster_changed.push(item_id.to_string());
    }
    /// Tests: arm and press the toast's Undo for `item_id` (what a click on "Undo" does).
    pub fn press_poster_undo(&mut self, item_id: &str, admin: bool, previous: Option<Vec<u8>>) {
        self.undo = Some(PosterUndo { item_id: item_id.to_string(), admin, previous, display_id: None, at: 0.0 });
        self.undo_poster();
    }
    /// Tests: `(item id, Primary ImageTag)` of the posters the top screen shows.
    pub fn top_poster_tags(&self) -> Vec<(String, Option<String>)> {
        self.stack.last().map(|(_, s)| s.poster_tags()).unwrap_or_default()
    }
    /// PP-4: the signed-in user's personal poster for `item_id` (tests).
    pub fn poster_override(&self, item_id: &str) -> Option<String> {
        self.poster_overrides.get(item_id).cloned()
    }
    /// NAV: names of the screens on the stack, bottom first (tests, scripts).
    pub fn history(&self) -> Vec<&'static str> {
        self.stack.iter().map(|(_, s)| s.name()).collect()
    }
    /// NAV: names of the screens Forward would re-open, nearest first.
    pub fn forward_history(&self) -> Vec<&'static str> {
        self.forward.iter().rev().map(|(_, s)| s.name()).collect()
    }

    /// Window requests made during the last frame (taken: the next frame starts empty).
    pub fn take_shell_requests(&mut self) -> crate::platform::ShellRequests {
        std::mem::take(&mut self.shell)
    }

    pub fn screen_name(&self) -> &'static str {
        self.stack.last().map(|(_, s)| s.name()).unwrap_or("none")
    }
    pub fn shows_video(&self) -> bool {
        self.stack.last().map(|(_, s)| s.shows_video()).unwrap_or(false)
    }
    /// No data loads or image loads pending (scripts wait for this before a screenshot).
    pub fn is_idle(&self) -> bool {
        self.tasks.pending() == 0 && self.images.pending() == 0
    }
    pub fn now(&self) -> f64 {
        match self.fixed_step {
            Some(_) => self.fixed_now,
            None => self.started.elapsed().as_secs_f64() + self.skew,
        }
    }
    /// Recording: `Some(step)` makes every frame exactly `step` seconds long for screens that animate on `cx.now`;
    /// `None` goes back to wall time (the clock only ever moves forward).
    pub fn set_fixed_step(&mut self, step: Option<f64>) {
        self.fixed_now = self.now().max(self.fixed_now);
        self.fixed_step = step;
    }
    /// Move the app clock forward (render scripts: `fast-forward`), e.g. to reach the screensaver without waiting.
    pub fn advance_clock(&mut self, secs: f64) {
        self.skew += secs;
        self.fixed_now += secs;
    }
    /// RR8 / P-USER-1: the shell's file picker chose `path`; the top screen gets it as `picked-file`.
    pub fn file_picked(&mut self, path: String) {
        self.picked.push(path);
    }
    /// RR8 / P-USER-2: the screensaver is up.
    pub fn screensaver_active(&self) -> bool {
        self.saver.is_some()
    }

    /// One frame: results → keys → draw → navigation.
    pub fn frame(&mut self, ctx: &egui::Context, player: &mut dyn Player, keys: &[Key]) {
        if !self.themed {
            self.theme.apply(ctx);
            // NAV: everything clickable shows a hand cursor.
            ctx.style_mut(|s| s.visuals.interact_cursor = Some(egui::CursorIcon::PointingHand));
            self.themed = true;
            // Fonts set now only bind at the next pass (named Inter weights would panic): draw from then.
            ctx.request_repaint();
            return;
        }
        // P-USER-2: a changed theme choice applies at once.
        let want = Theme::for_choice(self.settings.theme);
        if want != self.theme {
            self.theme = want;
            self.theme.apply_style(ctx);
            ctx.style_mut(|s| s.visuals.interact_cursor = Some(egui::CursorIcon::PointingHand));
        }
        // NAV: focus marks follow the last input — keys turn them on, the mouse moving, clicking or wheeling turns them
        // off. In mouse mode the focused item no longer scrolls itself into view (it would undo the wheel).
        if keys.iter().any(|k| matches!(k, Key::Up | Key::Down | Key::Left | Key::Right | Key::Enter)) {
            self.keyboard = true;
        } else if ctx.input(|i| {
            i.pointer.delta() != egui::Vec2::ZERO || i.pointer.any_pressed() || i.raw_scroll_delta != egui::Vec2::ZERO
        }) {
            self.keyboard = false;
        }
        // While the sidebar holds the keyboard focus, the screen shows none (one focused thing at a time).
        ui::shell::set_keyboard_focus(ctx, self.keyboard && self.side.is_none());
        self.images.begin_frame(ctx);
        let mut nav = vec![];
        if let Some(step) = self.fixed_step {
            self.fixed_now += step;
        }
        let now = self.now();
        let input = !keys.is_empty()
            || ctx.input(|i| {
                i.pointer.delta() != egui::Vec2::ZERO
                    || i.pointer.any_pressed()
                    || i.events.iter().any(|e| matches!(e, egui::Event::Text(_) | egui::Event::MouseWheel { .. }))
            });
        let mut keys = keys;
        if input {
            self.last_input = now;
            if self.saver.take().is_some() {
                keys = &[]; // the key that ends the screensaver does nothing else
            }
        }
        if self.saver.is_none() {
            let over_player = self.stack.last().map(|(_, s)| s.shows_video()).unwrap_or(false);
            let (mode, mins) = (self.settings.screensaver, self.settings.screensaver_minutes);
            if self.client.is_some() && ui::screensaver::idle_due(mode, mins, self.last_input, now, over_player) {
                self.saver = Some(ui::screensaver::Overlay::new(mode, now));
            }
        }
        if self.saver.as_ref().is_some_and(|s| s.wants_items()) {
            if let Some(c) = self.client.clone() {
                if let Some(s) = self.saver.as_mut() {
                    s.loaded = true;
                }
                self.tasks.spawn(SAVER_SCREEN, SAVER_TAG, move || {
                    c.backdrop_items(ui::screensaver::Overlay::slides()).unwrap_or_default()
                });
            }
        }
        let done = self.tasks.drain();
        let done: Vec<_> = done
            .into_iter()
            .filter_map(|d| {
                if d.screen == SAVER_SCREEN && d.tag == SAVER_TAG {
                    if let (Ok(items), Some(s)) = (d.payload.downcast::<Vec<crate::api::Item>>(), self.saver.as_mut()) {
                        s.set_items(*items);
                    }
                    return None;
                }
                if d.screen == ADMIN_SCREEN && d.tag == ADMIN_TAG {
                    if let Ok(b) = d.payload.downcast::<bool>() {
                        self.admin = *b;
                    }
                    None
                } else if d.screen == ADMIN_SCREEN && d.tag == REMEMBER_SUBS_TAG {
                    // FIX-4: kept per user id with the settings; only applied if that user is still signed in.
                    if let Ok(r) = d.payload.downcast::<Option<(String, bool)>>() {
                        let cur = self.client.as_ref().and_then(|c| c.session.as_ref()).map(|s| s.user_id.clone());
                        if let Some((uid, on)) = *r {
                            if Some(&uid) == cur.as_ref() && self.settings.remember_subs.get(&uid) != Some(&on) {
                                self.settings.remember_subs.insert(uid, on);
                                let _ = self.settings.save(self.store.as_ref());
                            }
                        }
                    }
                    None
                } else if d.screen == ADMIN_SCREEN && d.tag == VIEWS_TAG {
                    if let Ok(v) = d.payload.downcast::<Vec<crate::api::Item>>() {
                        self.views = *v;
                    }
                    None
                } else if d.screen == PICKS_SCREEN && d.tag == PICKS_TAG {
                    // PP-4: the server's copy of this user's picks; only applied if that user is still signed in.
                    if let Ok(r) = d.payload.downcast::<Option<(Option<String>, HashMap<String, String>)>>() {
                        let cur = self.client.as_ref().and_then(|c| c.session.as_ref()).map(|s| s.user_id.clone());
                        if let Some((Some(uid), picks)) = *r {
                            if Some(&uid) == cur.as_ref() {
                                self.poster_overrides = picks.clone();
                                self.settings.poster_picks.insert(uid, picks);
                                let _ = self.settings.save(self.store.as_ref());
                            }
                        }
                    }
                    None
                } else if d.screen == UNDO_SCREEN && d.tag == UNDO_TAG {
                    // PP-5: the undo finished. An admin undo changed the shared image (refresh grids); a
                    // viewer undo just clears the local override.
                    if let Ok(Ok((item_id, admin))) =
                        d.payload.downcast::<Result<(String, bool), ApiError>>().map(|b| *b)
                    {
                        if admin {
                            self.poster_changed.push(item_id);
                            self.refresh_top = true;
                        } else {
                            self.poster_overrides.remove(&item_id);
                            if let Some(uid) =
                                self.client.as_ref().and_then(|c| c.session.as_ref()).map(|s| s.user_id.clone())
                            {
                                self.settings.poster_picks.insert(uid, self.poster_overrides.clone());
                            }
                            let _ = self.settings.save(self.store.as_ref());
                        }
                    }
                    None
                } else {
                    Some(d)
                }
            })
            .collect();
        if std::mem::take(&mut self.refresh_top) {
            // An admin Undo finished: the visible screen re-reads now, not on its next resume.
            self.resume_top(player);
        }
        let Some((top_id, _)) = self.stack.last() else { return };
        let top_id = *top_id;
        ui::parts::set_tv(ctx, !self.opts.desktop);
        let tv_keys;
        if !self.opts.desktop {
            tv_keys = self.tv_keys(ctx, top_id, now, keys);
            keys = &tv_keys;
        }
        crate::ui::at_details::set_desktop(ctx, self.opts.desktop);
        let sidebar = self.opts.desktop && self.shell_on();
        if !sidebar {
            self.side = None;
            self.confirm = None;
        }
        // NAV: while the sidebar has keyboard focus it takes the keys; the rest go to the screen once it lets go.
        let keys = if self.confirm.is_some() {
            // UX D30: the Sign out confirmation takes every key (Cancel is focused first).
            self.confirm_keys(keys, &mut nav);
            vec![]
        } else if self.side.is_some() {
            self.sidebar_keys(keys, &mut nav)
        } else {
            keys.to_vec()
        };
        let held = std::mem::take(&mut self.keys_held);
        let mut enter_side = false;
        if sidebar {
            self.draw_shell(ctx, &mut nav);
        }
        {
            let (id, screen) = self.stack.last_mut().unwrap();
            let mut cx = Cx {
                theme: &self.theme,
                client: &mut self.client,
                settings: &mut self.settings,
                store: &self.store,
                tasks: &self.tasks,
                images: &mut self.images,
                player,
                opts: &self.opts,
                admin: self.admin,
                poster_overrides: &mut self.poster_overrides,
                poster_changed: &mut self.poster_changed,
                poster_gen: self.poster_gen,
                undo: &mut self.undo,
                keys_down: &self.keys_down,
                shell: &mut self.shell,
                now,
                sidebar,
                screen_id: *id,
                nav: &mut nav,
            };
            // R-STATE-3: results for screens that are no longer on top are dropped; screens reload in on_resume.
            for d in done.into_iter().filter(|d| d.screen == top_id) {
                screen.on_done(&mut cx, d.tag, d.payload);
            }
            for path in std::mem::take(&mut self.picked) {
                screen.on_done(&mut cx, "picked-file", Box::new(path));
            }
            for &k in &keys {
                if k == Key::Left {
                    crate::nav::take_left_edge();
                }
                let used = screen.on_key(&mut cx, k);
                // NAV: a single (not held) Left that stopped at the screen's left edge steps into the sidebar.
                if k == Key::Left && sidebar && !held && crate::nav::take_left_edge() && cx.nav.is_empty() {
                    enter_side = true;
                    break;
                }
                if !used && cx.nav.is_empty() {
                    match k {
                        Key::Back => cx.pop(),
                        Key::Forward => cx.forward(),
                        Key::Search if screen.name() != "search" && cx.client.is_some() => {
                            cx.section(Some(ui::search::screen()))
                        }
                        _ => {}
                    }
                }
            }
            if let Some(saver) = &self.saver {
                let frame = egui::Frame::NONE.fill(cx.theme.bg);
                let (t, now) = (cx.theme, cx.now);
                egui::CentralPanel::default()
                    .frame(frame)
                    .show(ctx, |ui| saver.ui(ui, t, cx.client.as_ref(), cx.images, now));
                self.images.end_frame(ctx);
                self.client_changed();
                self.apply_nav(nav, player);
                return;
            }
            let theme_bg = cx.theme.bg;
            let margin = egui::Margin::symmetric(cx.theme.margin as i8, 0);
            let frame = if screen.shows_video() {
                egui::Frame::NONE
            } else {
                egui::Frame::NONE.fill(theme_bg).inner_margin(margin)
            };
            // FIX-4: with the TV keyboard open the page ends above it (the keyboard is painted over that strip).
            ui::keyboard::set_open(ctx, self.tv_kb.is_some() && !self.opts.desktop);
            if let Some(kb) = self.tv_kb.as_ref().filter(|_| !self.opts.desktop) {
                let h = ui::keyboard::reserved_height(cx.theme, kb.layout().len());
                egui::TopBottomPanel::bottom("tv-kb-reserve")
                    .frame(egui::Frame::NONE.fill(theme_bg))
                    .exact_height(h)
                    .show_separator_line(false)
                    .show(ctx, |_| {});
            }
            egui::CentralPanel::default().frame(frame).show(ctx, |ui| screen.ui(ui, &mut cx));
        }
        self.draw_poster_toast(ctx);
        if enter_side {
            // Land on the section that is open now (Home when nothing above it matches).
            let places = ui::shell::entries(&self.views, self.admin);
            let here = self.current_place().unwrap_or(ui::shell::Place::Home);
            self.side = Some(places.iter().position(|p| *p == here).unwrap_or(0));
        }
        if !self.opts.desktop {
            self.tv_keyboard_ui(ctx);
        }
        self.images.end_frame(ctx);
        self.client_changed();
        self.apply_nav(nav, player);
    }

    /// UXP5: TV build — the keys for this frame with the on-screen keyboard applied. Enter on a focused text field
    /// opens the keyboard (Search opens it on arrival); while open it takes every key and types into the field as
    /// egui text events. Down from its last row closes it and goes on to the screen.
    fn tv_keys(&mut self, ctx: &egui::Context, top_id: u64, now: f64, keys: &[Key]) -> Vec<Key> {
        use ui::keyboard::{Effect, KeyboardState};
        // Text arriving from outside (a USB / Bluetooth keyboard on the TV) means no remote-only typing: stand down.
        if ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Text(_)))) {
            self.tv_text_at = Some(now);
        }
        let Some(field) = self.tv_field.clone() else {
            self.tv_kb = None;
            return keys.to_vec();
        };
        // A physical keyboard typed recently: leave it the field (its Enter submits). After a quiet spell
        // Enter on a field opens the on-screen keyboard again.
        if physical_typing(self.tv_text_at, now) && self.tv_kb.is_none() {
            return keys.to_vec();
        }
        // FIX-2 #1d: a screen asked for the keyboard on its focused field (e.g. a picked user's password).
        if self.tv_kb.is_none() && ui::keyboard::take_request(ctx) {
            self.tv_kb = Some(self.tv_kb_open(top_id, &field));
        }
        if self.tv_kb.is_none() && self.screen_name() == "search" && self.tv_kb_auto != top_id {
            self.tv_kb_auto = top_id;
            self.tv_kb = Some(KeyboardState::for_field(&field));
        }
        let mut out = vec![];
        for &k in keys {
            let Some(kb) = self.tv_kb.as_mut() else {
                if k == Key::Enter {
                    self.tv_kb = Some(self.tv_kb_open(top_id, &field));
                } else {
                    out.push(k);
                }
                continue;
            };
            match kb.press_at(k, Some(now)) {
                e @ (Effect::Type(_) | Effect::Delete | Effect::Clear) => {
                    ui::keyboard::apply_text(ctx, kb, &field.text, e);
                }
                Effect::None => {}
                Effect::Close => self.tv_kb_close(top_id, &field),
                Effect::Leave(k) => {
                    self.tv_kb_close(top_id, &field);
                    out.push(k);
                }
            }
        }
        out
    }

    /// FIX-5 R2/R3: the keyboard for `field` — on the kept key if it was closed on this screen and field, else on "a".
    fn tv_kb_open(&self, top_id: u64, field: &ui::keyboard::TvField) -> ui::keyboard::KeyboardState {
        match self
            .tv_kb_saved
            .iter()
            .find(|(id, hint, pw, _)| *id == top_id && *hint == field.hint && *pw == field.password)
        {
            Some((.., saved)) => ui::keyboard::KeyboardState::for_field_resume(field, saved),
            None => ui::keyboard::KeyboardState::for_field(field),
        }
    }

    /// FIX-5 R1: close the keyboard, keeping where it was left.
    fn tv_kb_close(&mut self, top_id: u64, field: &ui::keyboard::TvField) {
        if let Some(kb) = self.tv_kb.take() {
            self.tv_kb_saved
                .retain(|(id, hint, pw, _)| *id == top_id && !(*hint == field.hint && *pw == field.password));
            self.tv_kb_saved.push((top_id, field.hint.clone(), field.password, kb));
        }
    }

    /// UXP5: draw the on-screen keyboard (a click on a key counts as Enter on it) and remember which field is up.
    fn tv_keyboard_ui(&mut self, ctx: &egui::Context) {
        self.tv_field = ui::keyboard::take_field(ctx);
        let (Some(kb), Some(field)) = (self.tv_kb.as_mut(), self.tv_field.as_ref()) else {
            return;
        };
        if let Some((row, col)) = ui::keyboard::draw(ctx, &self.theme, kb, field) {
            (kb.row, kb.col) = (row, col);
            match kb.activate() {
                ui::keyboard::Effect::Close => {
                    let field = field.clone();
                    if let Some(&(top_id, _)) = self.stack.last() {
                        self.tv_kb_close(top_id, &field);
                    }
                    self.tv_kb = None;
                }
                e => {
                    ui::keyboard::apply_text(ctx, kb, &field.text, e);
                }
            }
            ctx.request_repaint();
        }
    }

    /// NAV: the sidebar + toolbar show on signed-in viewer screens, never over the player or the sign-in flow.
    fn shell_on(&self) -> bool {
        let signed_in = self.client.as_ref().is_some_and(|c| c.session.is_some());
        let Some((_, top)) = self.stack.last() else { return false };
        signed_in && !top.shows_video() && !matches!(top.name(), "connect" | "signin" | "quickconnect" | "forgot")
    }

    /// NAV: draws the sidebar and toolbar (egui side/top panels, before the screen's central panel) and turns a
    /// click into navigation.
    /// UX D30: Left/Right pick Cancel or Sign out, Enter acts, Back cancels.
    fn confirm_keys(&mut self, keys: &[Key], nav: &mut Vec<Nav>) {
        for &k in keys {
            let Some(f) = self.confirm else { return };
            match k {
                Key::Left => self.confirm = Some(0),
                Key::Right => self.confirm = Some(1),
                Key::Back => self.confirm = None,
                Key::Enter => {
                    self.confirm = None;
                    if f == 1 {
                        self.leave_session(true, nav);
                    }
                }
                _ => {}
            }
        }
    }

    /// NAV: the sidebar place of what is open now (the screen just above Home decides).
    fn current_place(&self) -> Option<ui::shell::Place> {
        let second = self.stack.get(1).map(|(_, s)| (s.name(), s.crumbs()));
        ui::shell::place_of(second.as_ref().map(|(n, c)| (*n, c.as_slice())), &self.views)
    }

    /// NAV: keys while the sidebar has focus — Up/Down move, Enter opens, Right / Back leave it (the screen keeps
    /// its focus). Returns the keys left over for the screen.
    fn sidebar_keys(&mut self, keys: &[Key], nav: &mut Vec<Nav>) -> Vec<Key> {
        let places = ui::shell::entries(&self.views, self.admin);
        let mut rest = vec![];
        for &k in keys {
            let Some(i) = self.side else {
                rest.push(k);
                continue;
            };
            match k {
                Key::Up => self.side = Some(i.saturating_sub(1)),
                Key::Down => self.side = Some((i + 1).min(places.len().saturating_sub(1))),
                Key::Enter => {
                    if let Some(p) = places.get(i).cloned() {
                        nav.push(Nav::Section(ui::shell::open(p, &self.views)));
                    }
                    self.side = None;
                }
                Key::Right | Key::Back => self.side = None,
                Key::Left => {}
                other => rest.push(other),
            }
        }
        rest
    }

    fn draw_shell(&mut self, ctx: &egui::Context, nav: &mut Vec<Nav>) {
        let entry = self.settings.last();
        let user = entry.and_then(|e| e.session.as_ref()).map(|s| s.user_name.clone()).unwrap_or_default();
        let crumbs: Vec<Vec<String>> =
            self.stack.iter().skip(1).map(|(_, s)| ui::shell::crumbs_of(s.as_ref())).collect();
        let view = ui::shell::View {
            views: &self.views,
            user: &user,
            admin: self.admin,
            collapsed: self.settings.sidebar_collapsed,
            place: self.current_place(),
            can_back: self.stack.len() > 1,
            can_forward: !self.forward.is_empty(),
            crumbs: &crumbs,
            focus: self.side,
            confirm: self.confirm,
        };
        let Some(action) = ui::shell::draw(ctx, &self.theme, &view) else { return };
        self.side = None;
        use ui::shell::Action;
        match action {
            Action::Back => nav.push(Nav::Pop),
            Action::Forward => nav.push(Nav::Forward),
            Action::Crumb(i) => nav.push(Nav::PopTo(i + 2)),
            Action::Open(place) => nav.push(Nav::Section(ui::shell::open(place, &self.views))),
            Action::Collapse => {
                self.settings.sidebar_collapsed = !self.settings.sidebar_collapsed;
                let _ = self.settings.save(self.store.as_ref());
            }
            Action::SignOut => self.confirm = Some(0),
            Action::ConfirmHover(i) => self.confirm = Some(i),
            Action::ConfirmPick(i) => {
                self.confirm = None;
                if i == 1 {
                    self.leave_session(true, nav);
                }
            }
            Action::SwitchUser => self.leave_session(false, nav),
        }
    }

    /// Switch user / Sign out (`sign_out`: also ends the session on the server): forget this server's session and
    /// open sign-in on the same server.
    fn leave_session(&mut self, sign_out: bool, nav: &mut Vec<Nav>) {
        let Some(mut c) = self.client.clone() else { return };
        if sign_out {
            let mut server = c.clone();
            self.tasks.spawn(ADMIN_SCREEN, "signout", move || {
                let _ = server.sign_out();
            });
        }
        c.session = None;
        if let Some(id) = self.settings.last_server.clone() {
            self.settings.forget_session(&id);
            let _ = self.settings.save(self.store.as_ref());
        }
        self.client = Some(c.clone());
        nav.push(Nav::Reset(ui::signin::screen(c, None)));
    }

    fn client_changed(&mut self) {
        let key = self.client.as_ref().map(|c| (c.base.clone(), c.session.as_ref().map(|s| s.token.clone())));
        if key != self.client_key {
            self.client_key = key;
            self.images.set_client(self.client.clone());
            self.admin = false;
            self.views.clear();
            // PP-4: personal poster picks belong to the user (settings keep one map per user id). At sign-in the
            // local copy shows at once, then the server's DisplayPreferences (the truth, shared by all this
            // user's devices) replace it.
            let uid = self.client.as_ref().and_then(|c| c.session.as_ref()).map(|s| s.user_id.clone());
            self.poster_overrides = uid.and_then(|u| self.settings.poster_picks.get(&u).cloned()).unwrap_or_default();
            if let Some(c) = self.client.clone() {
                if c.session.is_some() {
                    let v = c.clone();
                    self.tasks.spawn(ADMIN_SCREEN, VIEWS_TAG, move || v.views().unwrap_or_default());
                }
                if let Some(uid) = c.session.as_ref().map(|s| s.user_id.clone()) {
                    let r = c.clone();
                    self.tasks.spawn(ADMIN_SCREEN, REMEMBER_SUBS_TAG, move || {
                        r.remember_subtitle_selections().ok().map(|on| (uid, on))
                    });
                }
                if c.session.is_some() {
                    let p = c.clone();
                    self.tasks.spawn(PICKS_SCREEN, PICKS_TAG, move || {
                        p.display_preferences(crate::api::poster::POSTER_PREFS_ID)
                            .map(|d| (p.session.as_ref().map(|s| s.user_id.clone()), all_poster_prefs(&d.custom_prefs)))
                            .ok()
                    });
                }
                self.tasks.spawn(ADMIN_SCREEN, ADMIN_TAG, move || c.is_admin().unwrap_or(false));
            }
        }
    }

    /// PP-5: run the pending poster Undo on the worker pool; the result is drained in `frame`.
    fn undo_poster(&mut self) {
        let Some(u) = self.undo.take() else { return };
        let client = self.client.clone().expect("signed in");
        let item_id = u.item_id.clone();
        let admin = u.admin;
        let previous = u.previous;
        let display_id = u.display_id;
        self.tasks.spawn(UNDO_SCREEN, UNDO_TAG, move || -> Result<(String, bool), ApiError> {
            if admin {
                match previous {
                    Some(bytes) => client.set_item_image(&item_id, &bytes)?,
                    None => client.delete_item_image(&item_id)?,
                }
            } else {
                let dpid = display_id
                    .filter(|d| !d.is_empty())
                    .unwrap_or_else(|| crate::api::poster::POSTER_PREFS_ID.to_string());
                client.set_poster_pref(&dpid, &item_id, None)?;
            }
            Ok((item_id, admin))
        });
    }

    /// PP-5: the app-level "Poster changed · Undo" toast, drawn after the screen so it survives leaving the picker.
    fn draw_poster_toast(&mut self, ctx: &egui::Context) {
        const TOAST_SECS: f64 = 8.0;
        if self.undo.is_none() {
            return;
        }
        let now = self.now();
        if now - self.undo.as_ref().map(|u| u.at).unwrap_or(now) >= TOAST_SECS {
            self.undo = None;
            return;
        }
        let t = &self.theme;
        let mut clicked = false;
        egui::Area::new(egui::Id::new("poster-undo-toast"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-t.row_gap, -t.row_gap))
            .show(ctx, |ui| {
                let font = t.font_w(t.body_size, crate::theme::Weight::SemiBold);
                let g = ui.fonts(|f| {
                    f.layout_no_wrap("Poster changed".to_string(), font.clone(), egui::Color32::PLACEHOLDER)
                });
                let ug = ui.fonts(|f| f.layout_no_wrap("Undo".to_string(), font, egui::Color32::PLACEHOLDER));
                let w = g.size().x + 2.0 * t.pill_pad + t.pill_pad + ug.size().x + t.chip_pad * 2.0 + t.pill_pad;
                let h = t.pill_h;
                let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
                ui.painter().rect_filled(rect, h / 2.0, t.menu_fill);
                ui.painter().galley(rect.left_center() + egui::vec2(t.pill_pad, -g.size().y / 2.0), g, t.text);
                let undo_rect = egui::Rect::from_min_size(
                    egui::pos2(rect.right() - t.pill_pad - ug.size().x - t.chip_pad * 2.0, rect.top()),
                    egui::vec2(ug.size().x + t.chip_pad * 2.0, h),
                );
                let resp = ui.interact(undo_rect, ui.id().with("pp-undo"), egui::Sense::click());
                ui.painter().rect_filled(undo_rect, h / 2.0, t.focus_fill);
                ui.painter().galley(undo_rect.center() - ug.size() / 2.0, ug, t.focus_ink);
                clicked = resp.clicked();
            });
        if clicked {
            self.undo_poster();
        }
        ctx.request_repaint();
    }

    fn apply_nav(&mut self, nav: Vec<Nav>, player: &mut dyn Player) {
        for n in nav {
            match n {
                Nav::Push(s) => self.push(s),
                Nav::Pop => {
                    if self.stack.len() > 1 {
                        self.back_one(player);
                        self.resume_top(player);
                    }
                }
                Nav::Reset(s) => {
                    while !self.stack.is_empty() {
                        self.leave_top(player);
                        self.stack.pop();
                    }
                    let home = s.name() == "home";
                    self.push(s);
                    // Design galleries that need a signed-in client open on top of Home (`--screen gallery-…`).
                    if let Some(g) = self.opts.start_screen.as_deref().filter(|_| home).and_then(ui::gallery::signed_in)
                    {
                        self.push(g);
                    }
                }
                Nav::Forward => {
                    if let Some(s) = self.forward.pop() {
                        self.stack.push(s);
                        self.resume_top(player);
                    }
                }
                Nav::PopTo(n) => {
                    if self.stack.len() > n.max(1) {
                        while self.stack.len() > n.max(1) {
                            self.back_one(player);
                        }
                        self.resume_top(player);
                    }
                }
                Nav::Section(s) => {
                    while self.stack.len() > 1 {
                        self.leave_top(player);
                        self.stack.pop();
                    }
                    match s {
                        Some(s) => self.push(s),
                        None => {
                            self.forward.clear();
                            self.resume_top(player);
                        }
                    }
                }
            }
        }
    }

    fn with_top(&mut self, player: &mut dyn Player, f: impl FnOnce(&mut dyn Screen, &mut Cx)) {
        let mut nav = vec![];
        let now = self.now();
        if let Some((id, screen)) = self.stack.last_mut() {
            let mut cx = Cx {
                theme: &self.theme,
                client: &mut self.client,
                settings: &mut self.settings,
                store: &self.store,
                tasks: &self.tasks,
                images: &mut self.images,
                player,
                opts: &self.opts,
                admin: self.admin,
                poster_overrides: &mut self.poster_overrides,
                poster_changed: &mut self.poster_changed,
                poster_gen: self.poster_gen,
                undo: &mut self.undo,
                keys_down: &self.keys_down,
                shell: &mut self.shell,
                now,
                sidebar: false,
                screen_id: *id,
                nav: &mut nav,
            };
            f(screen.as_mut(), &mut cx);
        }
        // navigation requested from on_leave/on_resume is ignored on purpose (no loops)
    }
    /// Back: the top screen leaves and waits in the forward history (a player is not kept: it reported stopped).
    fn back_one(&mut self, player: &mut dyn Player) {
        self.leave_top(player);
        if let Some(top) = self.stack.pop() {
            if !top.1.shows_video() {
                self.forward.push(top);
            }
        }
    }
    fn leave_top(&mut self, player: &mut dyn Player) {
        self.with_top(player, |s, cx| s.on_leave(cx));
    }
    fn resume_top(&mut self, player: &mut dyn Player) {
        if !self.poster_changed.is_empty() {
            self.poster_gen += 1;
            self.poster_changed.clear();
        }
        self.with_top(player, |s, cx| s.on_resume(cx));
    }

    /// Window closing: let every screen clean up (the player reports stopped).
    pub fn shutdown(&mut self, player: &mut dyn Player) {
        while !self.stack.is_empty() {
            self.leave_top(player);
            self.stack.pop();
        }
        let t = std::time::Instant::now();
        while self.tasks.pending() > 0 && t.elapsed() < std::time::Duration::from_secs(2) {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
}

/// Tests of a single screen: a `Cx` over a signed-in app and a `NullPlayer`.
#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use crate::platform::NullPlayer;
    use crate::settings::MemStorage;

    /// A signed-in app (user "u") over a `MemStorage` that outlives it: `restart` reads the settings back from
    /// storage as the next run of the app would.
    pub(crate) struct World {
        app: App,
        mem: Arc<MemStorage>,
        player: NullPlayer,
        nav: Vec<Nav>,
    }

    impl World {
        /// `remember`: the user's Jellyfin "Remember subtitle selections", as read at sign-in.
        pub(crate) fn signed_in(remember: bool) -> World {
            let mem = Arc::new(MemStorage::default());
            let mut app = App::new(mem.clone(), AppOptions::default());
            let d = DeviceInfo { name: "t".into(), id: "d".into(), version: "0".into() };
            let s = crate::api::Session { user_id: "u".into(), user_name: "t".into(), token: "tok".into() };
            app.client = Some(Client::with_session("http://h:8096", d, s));
            app.settings.remember_subs.insert("u".into(), remember);
            let _ = app.settings.save(app.store.as_ref());
            World { app, mem, player: NullPlayer::default(), nav: vec![] }
        }
        pub(crate) fn settings(&self) -> &Settings {
            &self.app.settings
        }
        /// The user's "Remember subtitle selections" as read at a later sign-in.
        pub(crate) fn set_remember(&mut self, on: bool) {
            self.app.settings.remember_subs.insert("u".into(), on);
            let _ = self.app.settings.save(self.app.store.as_ref());
        }
        pub(crate) fn restart(&mut self) {
            self.app.settings = Settings::load(self.mem.as_ref());
        }
        pub(crate) fn cx<R>(&mut self, f: impl FnOnce(&mut Cx) -> R) -> R {
            let a = &mut self.app;
            let mut cx = Cx {
                theme: &a.theme,
                client: &mut a.client,
                settings: &mut a.settings,
                store: &a.store,
                tasks: &a.tasks,
                images: &mut a.images,
                player: &mut self.player,
                opts: &a.opts,
                admin: a.admin,
                poster_overrides: &mut a.poster_overrides,
                poster_changed: &mut a.poster_changed,
                poster_gen: a.poster_gen,
                undo: &mut a.undo,
                keys_down: &a.keys_down,
                shell: &mut a.shell,
                now: 0.0,
                sidebar: false,
                screen_id: 1,
                nav: &mut self.nav,
            };
            f(&mut cx)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::NullPlayer;
    use crate::settings::MemStorage;

    struct Fake(&'static str, bool);
    impl Screen for Fake {
        fn name(&self) -> &'static str {
            self.0
        }
        fn ui(&mut self, _ui: &mut egui::Ui, _cx: &mut Cx) {}
        fn shows_video(&self) -> bool {
            self.1
        }
    }
    fn s(name: &'static str) -> Box<dyn Screen> {
        Box::new(Fake(name, false))
    }

    /// An app whose stack is `names` (bottom first).
    fn app(names: &[&'static str]) -> (App, NullPlayer) {
        let mut a = App::new(Arc::new(MemStorage::default()), AppOptions::default());
        let mut p = NullPlayer::default();
        a.apply_nav(vec![Nav::Reset(s(names[0]))], &mut p);
        a.apply_nav(names[1..].iter().map(|n| Nav::Push(s(n))).collect(), &mut p);
        (a, p)
    }

    /// A one-answer HTTP server: every request gets `body` as JSON.
    fn stub(body: String) -> String {
        use std::io::{BufRead, BufReader, Write};
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
        std::thread::spawn(move || {
            for conn in l.incoming() {
                let Ok(mut conn) = conn else { continue };
                let mut r = BufReader::new(conn.try_clone().unwrap());
                let mut h = String::new();
                while r.read_line(&mut h).is_ok_and(|n| n > 0) && h != "\r\n" {
                    h.clear();
                }
                let _ = write!(
                    conn,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        base
    }

    #[test]
    fn sign_in_reads_remember_subtitle_selections_into_the_saved_settings() {
        for on in [true, false] {
            let base = stub(format!(r#"{{"Id":"u","Configuration":{{"RememberSubtitleSelections":{on}}}}}"#));
            let mem = Arc::new(MemStorage::default());
            let mut a = App::new(mem.clone(), AppOptions::default());
            let mut p = NullPlayer::default();
            a.apply_nav(vec![Nav::Reset(s("home"))], &mut p);
            let d = DeviceInfo { name: "t".into(), id: "d".into(), version: "0".into() };
            let session = crate::api::Session { user_id: "u".into(), user_name: "t".into(), token: "tok".into() };
            a.client = Some(Client::with_session(&base, d, session));
            let ctx = egui::Context::default();
            let saved = || Settings::load(mem.as_ref()).remember_subs.get("u").copied();
            for _ in 0..500 {
                let _ = ctx.run(egui::RawInput::default(), |ctx| a.frame(ctx, &mut p, &[]));
                if saved().is_some() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            assert_eq!(saved(), Some(on), "RememberSubtitleSelections {on} read at sign-in and saved");
        }
    }

    #[test]
    fn back_then_forward_reopens_the_same_screen() {
        let (mut a, mut p) = app(&["home", "library", "movie"]);
        a.apply_nav(vec![Nav::Pop], &mut p);
        assert_eq!(a.history(), ["home", "library"]);
        assert_eq!(a.forward_history(), ["movie"]);
        a.apply_nav(vec![Nav::Pop], &mut p);
        assert_eq!(a.forward_history(), ["library", "movie"]);
        a.apply_nav(vec![Nav::Forward], &mut p);
        assert_eq!(a.history(), ["home", "library"]);
        assert_eq!(a.forward_history(), ["movie"]);
        a.apply_nav(vec![Nav::Forward, Nav::Forward], &mut p);
        assert_eq!(a.history(), ["home", "library", "movie"]);
        assert!(a.forward_history().is_empty());
    }

    #[test]
    fn opening_a_new_screen_clears_forward() {
        let (mut a, mut p) = app(&["home", "library", "movie"]);
        a.apply_nav(vec![Nav::Pop, Nav::Push(s("search"))], &mut p);
        assert_eq!(a.history(), ["home", "library", "search"]);
        assert!(a.forward_history().is_empty());
    }

    #[test]
    fn back_stops_at_the_first_screen() {
        let (mut a, mut p) = app(&["home"]);
        a.apply_nav(vec![Nav::Pop], &mut p);
        assert_eq!(a.history(), ["home"]);
        assert!(a.forward_history().is_empty());
    }

    #[test]
    fn breadcrumb_pops_to_a_screen_keeping_forward() {
        let (mut a, mut p) = app(&["home", "library", "show", "season"]);
        a.apply_nav(vec![Nav::PopTo(2)], &mut p);
        assert_eq!(a.history(), ["home", "library"]);
        assert_eq!(a.forward_history(), ["show", "season"]);
    }

    #[test]
    fn section_goes_back_to_home_then_opens() {
        let (mut a, mut p) = app(&["home", "library", "movie"]);
        a.apply_nav(vec![Nav::Pop], &mut p);
        a.apply_nav(vec![Nav::Section(Some(s("search")))], &mut p);
        assert_eq!(a.history(), ["home", "search"]);
        assert!(a.forward_history().is_empty());
        a.apply_nav(vec![Nav::Section(None)], &mut p);
        assert_eq!(a.history(), ["home"]);
    }

    #[test]
    fn the_player_is_not_kept_for_forward() {
        let (mut a, mut p) = app(&["home", "movie"]);
        a.apply_nav(vec![Nav::Push(Box::new(Fake("player", true))), Nav::Pop], &mut p);
        assert_eq!(a.history(), ["home", "movie"]);
        assert!(a.forward_history().is_empty());
    }

    #[test]
    fn back_and_forward_keys_drive_history() {
        let (mut a, mut p) = app(&["home", "library"]);
        let ctx = egui::Context::default();
        let mut run = |a: &mut App, keys: &[Key]| {
            let _ = ctx.run(egui::RawInput::default(), |ctx| a.frame(ctx, &mut p, keys));
        };
        run(&mut a, &[]); // the first frame only loads fonts
        run(&mut a, &[Key::Back]);
        assert_eq!(a.history(), ["home"]);
        run(&mut a, &[Key::Forward]);
        assert_eq!(a.history(), ["home", "library"]);
        // Search needs a signed-in client: without one the key does nothing.
        run(&mut a, &[Key::Search]);
        assert_eq!(a.history(), ["home", "library"]);
    }

    /// A screen with one row of three items, moved with `nav::move_focus` like real screens.
    struct Row(&'static str, crate::nav::Focus);
    impl Screen for Row {
        fn name(&self) -> &'static str {
            self.0
        }
        fn on_key(&mut self, _cx: &mut Cx, key: Key) -> bool {
            let g = [crate::nav::Group { len: 3, cols: 3, xs: vec![] }];
            self.1 = crate::nav::move_focus(&g, self.1, key);
            !matches!(key, Key::Back | Key::Forward | Key::Search)
        }
        fn ui(&mut self, _ui: &mut egui::Ui, _cx: &mut Cx) {}
    }

    #[test]
    fn a_single_left_at_the_edge_enters_the_sidebar_on_the_current_section() {
        let mut a = App::new(Arc::new(MemStorage::default()), AppOptions { desktop: true, ..Default::default() });
        let mut p = NullPlayer::default();
        let session = crate::api::Session { user_id: "u".into(), user_name: "tester".into(), token: "t".into() };
        a.client = Some(Client::with_session("http://127.0.0.1:9", a.device(), session));
        a.apply_nav(vec![Nav::Reset(s("home")), Nav::Push(Box::new(Row("favourites", Default::default())))], &mut p);
        let ctx = egui::Context::default();
        let mut run = |a: &mut App, keys: &[Key], held: bool| {
            a.keys_held = held;
            let _ = ctx.run(egui::RawInput::default(), |ctx| a.frame(ctx, &mut p, keys));
        };
        run(&mut a, &[], false);
        // Held Left (a burst, autorepeat) stays in the screen.
        run(&mut a, &[Key::Left], true);
        assert_eq!(a.side, None);
        // One press at the edge: the sidebar, on Favourites (Home, Search, Collections, Playlists, Favourites, user).
        run(&mut a, &[Key::Left], false);
        assert_eq!(a.side, Some(4));
        run(&mut a, &[Key::Down], false);
        assert_eq!(a.side, Some(5));
        run(&mut a, &[Key::Up, Key::Up, Key::Up], false);
        assert_eq!(a.side, Some(2));
        // Right goes back to the screen, which kept its focus.
        run(&mut a, &[Key::Right], false);
        assert_eq!(a.side, None);
        assert_eq!(a.history(), ["home", "favourites"]);
        // Back from the sidebar leaves it without popping the screen.
        run(&mut a, &[Key::Left], false);
        run(&mut a, &[Key::Back], false);
        assert_eq!((a.side, a.history()), (None, vec!["home", "favourites"]));
        // Enter opens the place: Up to Search → Search on top of Home.
        run(&mut a, &[Key::Left], false);
        run(&mut a, &[Key::Up, Key::Up, Key::Up, Key::Enter], false);
        assert_eq!(a.side, None);
        assert_eq!(a.history(), ["home", "search"]);
    }

    #[test]
    fn sign_out_from_the_sidebar_asks_first() {
        let mut a = App::new(Arc::new(MemStorage::default()), AppOptions { desktop: true, ..Default::default() });
        let mut p = NullPlayer::default();
        let session = crate::api::Session { user_id: "u".into(), user_name: "tester".into(), token: "t".into() };
        a.client = Some(Client::with_session("http://127.0.0.1:9", a.device(), session));
        a.apply_nav(vec![Nav::Reset(s("home"))], &mut p);
        let ctx = egui::Context::default();
        let mut run = |a: &mut App, keys: &[Key]| {
            let _ = ctx.run(egui::RawInput::default(), |ctx| a.frame(ctx, &mut p, keys));
        };
        run(&mut a, &[]);
        // The menu's Sign out only opens the confirmation, with Cancel focused; Back and Cancel keep the session.
        a.confirm = Some(0);
        run(&mut a, &[Key::Back]);
        assert_eq!((a.confirm, a.history()), (None, vec!["home"]));
        a.confirm = Some(0);
        run(&mut a, &[Key::Enter]);
        assert_eq!((a.confirm, a.history()), (None, vec!["home"]));
        // Right → Sign out, Enter → signed out, back on sign-in for the same server.
        a.confirm = Some(0);
        run(&mut a, &[Key::Right, Key::Enter]);
        assert_eq!(a.history(), ["signin"]);
        assert!(a.client.as_ref().is_some_and(|c| c.session.is_none()));
    }
}

/// FIX-1 #10: seconds without text from a physical keyboard before the TV's on-screen keyboard comes back.
pub const TV_TEXT_QUIET: f64 = 30.0;

/// True while a physical keyboard typed within the last `TV_TEXT_QUIET` seconds.
pub fn physical_typing(last_text: Option<f64>, now: f64) -> bool {
    last_text.is_some_and(|t| now - t < TV_TEXT_QUIET)
}

#[cfg(test)]
mod fix1_keyboard_tests {
    use super::*;

    #[test]
    fn physical_typing_lapses_after_a_quiet_spell() {
        assert!(!physical_typing(None, 100.0));
        assert!(physical_typing(Some(100.0), 110.0));
        assert!(!physical_typing(Some(100.0), 100.0 + TV_TEXT_QUIET));
    }
}
