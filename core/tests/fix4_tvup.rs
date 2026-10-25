//! FIX-4 F4-TVUP (FRICTION-5 #6): on TV, Up from the topmost row of a bar-opened library or Search pops to the bar.
use jf_core::nav::{move_focus, Focus, Group};
use jf_core::platform::Key;
use jf_core::ui::library::{up_key, UpKey};

fn row(len: usize) -> Group {
    Group { len, cols: len.max(1), xs: vec![] }
}

// Library group order: tabs, sort/view row, toggle, chips, play, A–Z, sections.
fn library(tabs: usize) -> Vec<Group> {
    vec![
        row(tabs),
        row(2),
        Group::default(),
        row(5),
        Group::default(),
        Group::default(),
        Group { len: 14, cols: 7, xs: vec![] },
    ]
}

fn up(groups: &[Group], from: Focus, tv: bool) -> UpKey {
    up_key(from, move_focus(groups, from, Key::Up), tv)
}

#[test]
fn up_from_tabs_never_moves_down() {
    // The first-row focus nav hands back after Up is never a lower row, on either platform; TV pops instead.
    let g = library(4);
    let from = Focus { group: 0, index: 2 };
    assert_eq!(up_key(from, Focus { group: 3, index: 0 }, true), UpKey::Pop, "a lower row is not taken");
    assert_eq!(up(&g, from, true), UpKey::Pop);
    for tv in [true, false] {
        for start in [Focus { group: 0, index: 1 }, Focus { group: 3, index: 4 }, Focus { group: 6, index: 9 }] {
            if let UpKey::Move(f) = up(&g, start, tv) {
                let above = f.group < start.group || (f.group == start.group && f.index < start.index);
                assert!(above || (!tv && f == start), "{start:?} -> {f:?}");
            }
        }
    }
}

#[test]
fn tv_up_from_top_pops_to_bar_on_tab() {
    // Top row: the tabs row, or the sort row when the library has a single tab (no tabs group).
    assert_eq!(up(&library(4), Focus { group: 0, index: 0 }, true), UpKey::Pop);
    assert_eq!(up(&library(0), Focus { group: 1, index: 1 }, true), UpKey::Pop);
}

#[test]
fn search_up_from_top_pops_to_bar() {
    // Search groups: field, then Top results.
    let g = vec![row(1), Group { len: 6, cols: 6, xs: vec![] }];
    assert_eq!(up(&g, Focus { group: 0, index: 0 }, true), UpKey::Pop);
    assert_eq!(
        up(&g, Focus { group: 1, index: 3 }, true),
        UpKey::Move(Focus { group: 0, index: 0 }),
        "results → field first"
    );
}

#[test]
fn grid_up_reaches_chips_then_tabs() {
    let g = library(4);
    // One press per row, as today: first grid row → chips → sort row → tabs → (TV) pop.
    let UpKey::Move(a) = up(&g, Focus { group: 6, index: 0 }, true) else { panic!("grid → chips") };
    assert_eq!(a.group, 3);
    let UpKey::Move(b) = up(&g, a, true) else { panic!("chips → sort row") };
    assert_eq!(b.group, 1);
    let UpKey::Move(c) = up(&g, b, true) else { panic!("sort row → tabs") };
    assert_eq!(c.group, 0);
    assert_eq!(up(&g, c, true), UpKey::Pop);
    // Second grid row goes to the first grid row, not to the chips.
    assert_eq!(up(&g, Focus { group: 6, index: 8 }, true), UpKey::Move(Focus { group: 6, index: 1 }));
}

#[test]
fn desk_up_unchanged() {
    let g = library(4);
    let top = Focus { group: 0, index: 1 };
    assert_eq!(up(&g, top, false), UpKey::Move(move_focus(&g, top, Key::Up)), "desk never pops");
    let mid = Focus { group: 3, index: 2 };
    assert_eq!(up(&g, mid, false), UpKey::Move(move_focus(&g, mid, Key::Up)));
}

// ---- the real App, signed in on the throwaway server (JF_TEST_URL; skipped without it) ----
use jf_core::app::{App, AppOptions};
use jf_core::platform::NullPlayer;
use jf_core::settings::MemStorage;
use std::sync::Arc;
use std::time::{Duration, Instant};

struct Rig {
    app: App,
    ctx: egui::Context,
    player: NullPlayer,
}

impl Rig {
    fn frame(&mut self, keys: &[Key], events: Vec<egui::Event>) {
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1920.0, 1080.0));
        let (app, player) = (&mut self.app, &mut self.player);
        let _ = self.ctx.run(egui::RawInput { screen_rect: Some(rect), events, ..Default::default() }, |ctx| {
            app.frame(ctx, player, keys)
        });
    }
    fn wait_screen(&mut self, name: &str) {
        let start = Instant::now();
        while self.app.screen_name() != name && start.elapsed() < Duration::from_secs(20) {
            self.frame(&[], vec![]);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(self.app.screen_name(), name);
    }
    fn settle(&mut self) {
        let start = Instant::now();
        while !self.app.is_idle() && start.elapsed() < Duration::from_secs(20) {
            self.frame(&[], vec![]);
            std::thread::sleep(Duration::from_millis(10));
        }
        for _ in 0..8 {
            self.frame(&[], vec![]);
        }
    }
    fn press(&mut self, key: Key) {
        self.frame(&[key], vec![]);
        self.frame(&[], vec![]);
    }
    /// Signed in as the viewer-type test user and on Home; `desktop` = the sidebar shell (desk), else the TV build.
    fn home(desktop: bool) -> Option<Rig> {
        let url = std::env::var("JF_TEST_URL").ok()?;
        let opts = AppOptions { device_name: "fix4-tvup".into(), desktop, ..Default::default() };
        let mut r = Rig {
            app: App::new(Arc::new(MemStorage::default()), opts),
            ctx: egui::Context::default(),
            player: NullPlayer::default(),
        };
        for _ in 0..5 {
            r.frame(&[], vec![]);
        }
        r.frame(&[], vec![egui::Event::Text(url)]);
        r.press(Key::Enter);
        r.wait_screen("signin");
        for _ in 0..5 {
            r.frame(&[], vec![]);
        }
        r.frame(&[], vec![egui::Event::Text("tester".into())]);
        r.press(Key::Down);
        r.frame(&[], vec![egui::Event::Text("test-pass-1".into())]);
        r.press(Key::Enter);
        r.wait_screen("home");
        // Home asks for its libraries on its first frames; give the bar time to list them before any key.
        for _ in 0..100 {
            r.frame(&[], vec![]);
            std::thread::sleep(Duration::from_millis(10));
        }
        r.settle();
        Some(r)
    }
}

/// U2 on the real screens: Movies from the bar, three Ups reach the chips, sort row and tabs and stay on the
/// library; the next Up pops to Home. (Reverting the production change leaves the screen on "library".)
#[test]
fn tv_library_up_from_tabs_returns_home_on_the_bar() {
    let Some(mut r) = Rig::home(false) else { return };
    r.press(Key::Up); // hero → the bar
    r.press(Key::Right); // Home → first library
    r.press(Key::Enter);
    r.wait_screen("library");
    r.settle();
    for n in 1..=3 {
        r.press(Key::Up);
        assert_eq!(r.app.screen_name(), "library", "Up {n} stays on the library");
    }
    r.press(Key::Up);
    assert_eq!(r.app.screen_name(), "home", "Up from the tabs row pops");
    // Home keeps the bar focus: Enter opens the same library again.
    r.press(Key::Enter);
    r.wait_screen("library");
}

#[test]
fn tv_search_up_returns_home() {
    let Some(mut r) = Rig::home(false) else { return };
    r.press(Key::Search);
    r.wait_screen("search");
    r.settle();
    for _ in 0..6 {
        if r.app.screen_name() == "home" {
            break;
        }
        r.press(Key::Up);
    }
    assert_eq!(r.app.screen_name(), "home", "Up out of Search's top reaches Home");
}

#[test]
fn desk_search_up_stays() {
    let Some(mut r) = Rig::home(true) else { return };
    r.press(Key::Search);
    r.wait_screen("search");
    r.settle();
    for _ in 0..6 {
        r.press(Key::Up);
    }
    assert_eq!(r.app.screen_name(), "search", "desk never pops on Up");
}

/// S1: the sort/view/Play all/Shuffle/Poster Flow buttons are one screen row. From every button on it, Down reaches the
/// chips (or grid) and Up comes back to the row; a button numbered below the chips' group (Play all) must not read as
/// "a row below" and pop. Only Up from the top row pops. Walked over the first libraries on the bar.
#[test]
fn tv_library_up_from_chips_stays_on_the_control_row() {
    for lib in 0..3 {
        let Some(mut r) = Rig::home(false) else { return };
        r.press(Key::Up); // hero → the bar
        for _ in 0..=lib {
            r.press(Key::Right); // Home → library `lib`
        }
        r.press(Key::Enter);
        if r.app.screen_name() != "library" {
            r.wait_screen("library");
        }
        r.settle();
        r.press(Key::Up); // grid → chips
        r.press(Key::Up); // chips → control row
        assert_eq!(r.app.screen_name(), "library", "library {lib}: reached the control row");
        for k in 0..8 {
            for _ in 0..8 {
                r.press(Key::Left);
            }
            for _ in 0..k {
                r.press(Key::Right);
            }
            r.press(Key::Down);
            r.press(Key::Up);
            assert_eq!(r.app.screen_name(), "library", "library {lib}, button {k}: Down then Up stays on the library");
        }
    }
}
