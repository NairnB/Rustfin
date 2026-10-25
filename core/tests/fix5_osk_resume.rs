//! FIX-5 (F5-OSK-RESUME): the TV keyboard comes back where you left it.
use jf_core::app::{App, AppOptions};
use jf_core::platform::{Key, NullPlayer};
use jf_core::settings::MemStorage;
use jf_core::theme::Theme;
use jf_core::ui::keyboard::{layout, panel_rect, KeyboardState, TvField};
use std::sync::Arc;

/// One frame: the focused key's label (the brightest white key plate, if the keyboard is up) and the painted texts.
fn frame(app: &mut App, ctx: &egui::Context, keys: &[Key]) -> (Option<String>, Vec<String>) {
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1920.0, 1080.0));
    let out = ctx.run(egui::RawInput { screen_rect: Some(rect), ..Default::default() }, |ctx| {
        app.frame(ctx, &mut NullPlayer::default(), keys)
    });
    focused_and_texts(&out)
}

/// Top of the keyboard panel on the 1920x1080 test screen.
fn kb_top() -> f32 {
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1920.0, 1080.0));
    panel_rect(&Theme::placeholder(), screen, layout(false, false).len()).top()
}

fn focused_and_texts(out: &egui::FullOutput) -> (Option<String>, Vec<String>) {
    let mut white = vec![];
    let mut texts = vec![];
    for s in &out.shapes {
        match &s.shape {
            egui::Shape::Rect(r)
                if r.rect.width() < 700.0
                    && r.fill.r() > 0x80
                    && r.rect.height() < 120.0
                    && r.rect.top() >= kb_top() =>
            {
                white.push((r.fill.r(), r.rect))
            }
            egui::Shape::Text(t) => texts.push((t.galley.text().to_string(), t.visual_bounding_rect().center())),
            _ => {}
        }
    }
    white.sort_by_key(|w| std::cmp::Reverse(w.0)); // a plate still fading out is dimmer than the lit one
    let focused = white.iter().find_map(|(_, w)| texts.iter().find(|(_, c)| w.contains(*c)).map(|(s, _)| s.clone()));
    (focused, texts.into_iter().map(|(s, _)| s).collect())
}

fn settle(app: &mut App, ctx: &egui::Context, keys: &[Key]) -> (Option<String>, Vec<String>) {
    let mut last = frame(app, ctx, keys);
    for _ in 0..30 {
        last = frame(app, ctx, &[]);
    }
    last
}

/// The connect screen on TV (its address field), keyboard not open.
fn connect_app() -> (App, egui::Context) {
    let app = App::new(
        Arc::new(MemStorage::default()),
        AppOptions {
            default_server: None,
            device_name: "osk-resume".into(),
            start_screen: None,
            desktop: false,
            poster_candidates: None,
            poster_fixture_dir: None,
        },
    );
    let ctx = egui::Context::default();
    ctx.set_fonts(Theme::fonts());
    let mut app = app;
    for _ in 0..6 {
        frame(&mut app, &ctx, &[]);
    }
    assert_eq!(app.screen_name(), "connect");
    (app, ctx)
}

fn shown(texts: &[String], s: &str) -> bool {
    texts.iter().any(|t| t == s)
}

/// Open the keyboard on connect, type "a" then move to the "-" key (letter row 3) and type it, leaving focus on "-".
fn typed_dash(app: &mut App, ctx: &egui::Context) {
    settle(app, ctx, &[Key::Enter]);
    settle(app, ctx, &[Key::Enter]); // types "a"
    for k in [Key::Down, Key::Down] {
        settle(app, ctx, &[k]);
    }
    for _ in 0..7 {
        settle(app, ctx, &[Key::Right]);
    }
    let (f, _) = settle(app, ctx, &[]);
    assert_eq!(f.as_deref(), Some("-"), "focus walked to the dash key");
    settle(app, ctx, &[Key::Enter]);
}

#[test]
fn reopen_same_field_resumes_key_and_layer() {
    let (mut app, ctx) = connect_app();
    typed_dash(&mut app, &ctx);
    settle(&mut app, &ctx, &[Key::Back]);
    let (f, texts) = settle(&mut app, &ctx, &[]);
    assert!(f.is_none() && !shown(&texts, "space"), "Back closes the keyboard");
    let (f, texts) = settle(&mut app, &ctx, &[Key::Enter]);
    assert!(shown(&texts, "space"), "Enter reopens it");
    assert_eq!(f.as_deref(), Some("-"), "reopened on the key it was left on");
    // The layer too: Up to the side keys' row, Left to the layer key, Enter (symbols on), Back, Enter.
    settle(&mut app, &ctx, &[Key::Up]);
    for _ in 0..12 {
        settle(&mut app, &ctx, &[Key::Left]);
    }
    let (_, texts) = settle(&mut app, &ctx, &[Key::Enter]);
    let (on_layer_key, _) = settle(&mut app, &ctx, &[]);
    assert!(shown(&texts, "?"), "symbols layer is up");
    settle(&mut app, &ctx, &[Key::Back]);
    let (f, texts) = settle(&mut app, &ctx, &[Key::Enter]);
    assert!(shown(&texts, "?"), "reopened on the symbols layer");
    assert_eq!(f, on_layer_key, "and on the layer key it was left on");
}

#[test]
fn reopen_after_leave_up_resumes() {
    let (mut app, ctx) = connect_app();
    typed_dash(&mut app, &ctx);
    for _ in 0..3 {
        settle(&mut app, &ctx, &[Key::Up]);
    }
    let (f, _) = settle(&mut app, &ctx, &[]);
    let top = f.expect("a key is lit on the digit row");
    assert!(top.chars().all(|c| c.is_ascii_digit()), "digit row: {top}");
    let (_, texts) = settle(&mut app, &ctx, &[Key::Up]);
    assert!(!shown(&texts, "space"), "Up from the top row leaves the keyboard (R4)");
    let (f, texts) = settle(&mut app, &ctx, &[Key::Enter]);
    assert!(shown(&texts, "space"), "Enter reopens it");
    assert_eq!(f.as_deref(), Some(top.as_str()), "reopened on the key it left from, not on a");
}

#[test]
fn empty_field_starts_on_a() {
    let (mut app, ctx) = connect_app();
    settle(&mut app, &ctx, &[Key::Enter]);
    settle(&mut app, &ctx, &[Key::Enter]); // types "a"
    settle(&mut app, &ctx, &[Key::Right]);
    settle(&mut app, &ctx, &[Key::Right]);
    settle(&mut app, &ctx, &[Key::Delete]); // the field is empty again
    settle(&mut app, &ctx, &[Key::Back]);
    let (f, texts) = settle(&mut app, &ctx, &[Key::Enter]);
    assert!(shown(&texts, "space"));
    assert_eq!(f.as_deref(), Some("a"), "nothing typed: a fresh keyboard");
}

// ---- the real App on the throwaway server (JF_TEST_URL; skipped without it) ----
use std::time::{Duration, Instant};

struct Rig {
    app: App,
    ctx: egui::Context,
}

impl Rig {
    fn frame(&mut self, keys: &[Key], events: Vec<egui::Event>) -> (Option<String>, Vec<String>) {
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1920.0, 1080.0));
        let app = &mut self.app;
        let out = self.ctx.run(egui::RawInput { screen_rect: Some(rect), events, ..Default::default() }, |ctx| {
            app.frame(ctx, &mut NullPlayer::default(), keys)
        });
        focused_and_texts(&out)
    }
    fn press(&mut self, key: Key) -> (Option<String>, Vec<String>) {
        self.frame(&[key], vec![]);
        let mut last = self.frame(&[], vec![]);
        for _ in 0..30 {
            last = self.frame(&[], vec![]);
        }
        last
    }
    fn wait_screen(&mut self, name: &str) {
        let start = Instant::now();
        while self.app.screen_name() != name && start.elapsed() < Duration::from_secs(20) {
            self.frame(&[], vec![]);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(self.app.screen_name(), name);
    }
    /// The sign-in form of the throwaway server (user name focused), TV build.
    fn signin() -> Option<Rig> {
        let url = std::env::var("JF_TEST_URL").ok()?;
        let opts = AppOptions { device_name: "fix5-osk".into(), ..Default::default() };
        let mut r = Rig { app: App::new(Arc::new(MemStorage::default()), opts), ctx: egui::Context::default() };
        r.ctx.set_fonts(Theme::fonts());
        for _ in 0..5 {
            r.frame(&[], vec![]);
        }
        r.frame(&[], vec![egui::Event::Text(url)]);
        r.press(Key::Enter);
        r.wait_screen("signin");
        r.app.advance_clock(60.0); // the address arrived as text: let the remote keyboard come back
        for _ in 0..8 {
            r.frame(&[], vec![]);
        }
        Some(r)
    }
}

#[test]
fn other_field_starts_on_a() {
    let Some(mut r) = Rig::signin() else { return };
    r.press(Key::Enter); // keyboard on the user name
    r.press(Key::Enter); // types "a"
    r.press(Key::Right);
    let (f, _) = r.press(Key::Right);
    assert_eq!(f.as_deref(), Some("c"), "walked to c on the user name");
    r.press(Key::Back);
    r.press(Key::Down); // the password field
    let (f, texts) = r.press(Key::Enter);
    assert!(shown(&texts, "space"), "keyboard up on the password");
    assert_eq!(f.as_deref(), Some("a"), "a different field starts on a");
    r.press(Key::Back);
    r.press(Key::Up); // back to the user name
    let (f, _) = r.press(Key::Enter);
    assert_eq!(f.as_deref(), Some("c"), "the user name still resumes on the key it was left on");
}

#[test]
fn search_auto_open_unchanged() {
    let Some(mut r) = Rig::signin() else { return };
    for _ in 0..3 {
        r.frame(&[], vec![]);
    }
    r.frame(&[], vec![egui::Event::Text("tester".into())]);
    r.press(Key::Down);
    r.frame(&[], vec![egui::Event::Text("test-pass-1".into())]);
    r.press(Key::Enter);
    r.app.advance_clock(60.0);
    r.wait_screen("home");
    for _ in 0..100 {
        r.frame(&[], vec![]);
        std::thread::sleep(Duration::from_millis(10));
    }
    r.press(Key::Search);
    r.wait_screen("search");
    let mut last = (None, vec![]);
    for _ in 0..10 {
        last = r.frame(&[], vec![]);
    }
    assert!(shown(&last.1, "space"), "Search opens the keyboard by itself");
    assert_eq!(last.0.as_deref(), Some("a"));
    // Type a letter, leave with Back, leave Search and come back: a new screen opens on "a" again.
    r.press(Key::Enter);
    r.press(Key::Right);
    r.press(Key::Back);
    r.press(Key::Back);
}

#[test]
fn name_field_shift_rule_on_resume() {
    let name = |text: &str| TvField { text: text.into(), password: false, hint: "Playlist name".into(), name: true };
    let user = |text: &str| TvField { text: text.into(), password: false, hint: "User name".into(), name: false };
    let mut saved = KeyboardState::for_field(&name("x"));
    saved.row = 3;
    saved.col = 4;
    let k = KeyboardState::for_field_resume(&name("Road "), &saved);
    assert!(k.shift && (k.row, k.col) == (3, 4), "after a space a name starts capitalised, on the kept key");
    let k = KeyboardState::for_field_resume(&name("Road"), &saved);
    assert!(!k.shift && (k.row, k.col) == (3, 4));
    let k = KeyboardState::for_field_resume(&user("guest "), &saved);
    assert!(!k.shift, "user names never capitalise");
}
