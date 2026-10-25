//! FIX-4 (F4-OSK): a TV keyboard you can read and type on — a digit row, bigger keys, one white key, a big typed line.
use jf_core::app::{App, AppOptions};
use jf_core::platform::{Key, NullPlayer};
use jf_core::settings::MemStorage;
use jf_core::theme::Theme;
use jf_core::ui::keyboard::{draw, layout, panel_rect, Cap, Effect, KeyboardState, TvField};
use std::sync::Arc;

fn chars(symbols: bool) -> String {
    layout(symbols, false)
        .iter()
        .flatten()
        .filter_map(|c| if let Cap::Char(c) = c.cap { Some(c) } else { None })
        .collect()
}

fn field(name: bool, password: bool, hint: &str) -> TvField {
    TvField { text: String::new(), password, hint: hint.into(), name }
}

#[test]
fn letter_layer_has_digit_row() {
    let rows = layout(false, false);
    let first: String = rows[0].iter().filter_map(|c| if let Cap::Char(c) = c.cap { Some(c) } else { None }).collect();
    assert_eq!(first, "1234567890");
    let second: String = rows[1].iter().filter_map(|c| if let Cap::Char(c) = c.cap { Some(c) } else { None }).collect();
    assert!(second.starts_with('a'));
}

#[test]
fn fleet_texts_need_no_layer_switch() {
    let on_letters = chars(false);
    for text in ["view-pass-1", "fleet-r41-1", "guest-3fa9c1", "hello-guest1", "tester", "test-pass-1"] {
        for ch in text.chars() {
            assert!(on_letters.contains(ch), "{ch} of {text} is not on the letter layer");
        }
    }
}

#[test]
fn panel_keeps_bottom_margin() {
    let t = Theme::placeholder();
    // 720p drawn at 1080 points high: 1 point = 2/3 px
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1920.0, 1080.0));
    let r = panel_rect(&t, screen, layout(false, false).len());
    assert!((screen.bottom() - r.bottom()) * 720.0 / 1080.0 >= 24.0, "{}", screen.bottom() - r.bottom());
    assert!(r.top() > screen.top() && r.left() >= screen.left() && r.right() <= screen.right());
}

#[test]
fn shift_stays_one_shot_and_says_on() {
    let mut k = KeyboardState::for_field(&field(false, false, "Search"));
    k.row = 1;
    k.col = 0;
    assert_eq!(k.focused_cap(), Cap::Shift);
    k.press(Key::Enter);
    assert!(k.shift);
    k.col = 1;
    assert_eq!(k.press(Key::Enter), Effect::Type('A'));
    assert!(!k.shift);
}

#[test]
fn name_field_capital_kept() {
    let k = KeyboardState::for_field(&field(true, false, "Playlist name"));
    assert!(k.shift);
    assert_eq!(k.focused_cap(), Cap::Char('A'));
    for f in [field(false, false, "User name"), field(false, true, "Password")] {
        let k = KeyboardState::for_field(&f);
        assert!(!k.shift);
        assert_eq!(k.focused_cap(), Cap::Char('a'));
    }
}

fn frame(app: &mut App, ctx: &egui::Context, keys: &[Key]) {
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1920.0, 1080.0));
    let _ = ctx.run(egui::RawInput { screen_rect: Some(rect), ..Default::default() }, |ctx| {
        app.frame(ctx, &mut NullPlayer::default(), keys)
    });
}

/// The strip the page gives up for the keyboard (the app's reserve panel), if one is up.
fn reserve(ctx: &egui::Context) -> Option<egui::Rect> {
    egui::containers::panel::PanelState::load(ctx, egui::Id::new("tv-kb-reserve")).map(|p| p.rect)
}

#[test]
fn page_ends_above_the_open_keyboard() {
    let mut app = App::new(
        Arc::new(MemStorage::default()),
        AppOptions {
            default_server: None,
            device_name: "kb-test".into(),
            start_screen: None,
            desktop: false,
            poster_candidates: None,
            poster_fixture_dir: None,
        },
    );
    let ctx = egui::Context::default();
    for _ in 0..6 {
        frame(&mut app, &ctx, &[]);
    }
    assert_eq!(app.screen_name(), "connect");
    assert!(reserve(&ctx).is_none(), "no keyboard: the page runs to the screen edge");
    frame(&mut app, &ctx, &[Key::Enter]);
    frame(&mut app, &ctx, &[]);
    let strip = reserve(&ctx).expect("keyboard open: the page leaves its strip");
    let kb = panel_rect(
        &Theme::placeholder(),
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1920.0, 1080.0)),
        layout(false, false).len(),
    );
    assert!(strip.top() <= kb.top() && strip.bottom() >= 1079.0, "strip {strip:?} must cover the keyboard {kb:?}");
}

/// The keyboard as painted for `f`: (key plates = rounded rect shapes with their fill, text sizes by string).
type Plates = Vec<(egui::Rect, egui::Color32)>;
type Texts = Vec<(String, f32)>;

fn painted(f: &TvField) -> (Plates, Texts) {
    let t = Theme::placeholder();
    let ctx = egui::Context::default();
    let kb = KeyboardState::for_field(f);
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1920.0, 1080.0));
    ctx.set_fonts(Theme::fonts());
    let _ = ctx.run(egui::RawInput { screen_rect: Some(rect), ..Default::default() }, |_| {});
    let mut out = None;
    for _ in 0..3 {
        out = Some(ctx.run(egui::RawInput { screen_rect: Some(rect), ..Default::default() }, |ctx| {
            draw(ctx, &t, &kb, f);
        }));
    }
    let (mut plates, mut texts) = (vec![], vec![]);
    for s in out.unwrap().shapes {
        match s.shape {
            egui::Shape::Rect(r) => plates.push((r.rect, r.fill)),
            egui::Shape::Text(tx) => {
                let size = tx.galley.job.sections[0].format.font_id.size;
                texts.push((tx.galley.text().to_string(), size));
            }
            _ => {}
        }
    }
    (plates, texts)
}

#[test]
fn key_height_at_least_1_5x() {
    let t = Theme::placeholder();
    let (plates, texts) = painted(&field(false, false, "User name"));
    // every key plate (the panel behind them is far wider) is at least 1.5x the old 40-point key
    let keys: Vec<_> = plates.iter().filter(|(r, _)| r.width() < 700.0).collect();
    assert!(keys.len() >= 40, "{}", keys.len());
    assert!(keys.iter().all(|(r, _)| r.height() >= 60.0 - 0.5), "key plates are 1.5x the old height");
    let label = texts.iter().find(|(s, _)| s == "a").expect("key a").1;
    assert!(label >= 1.5 * t.body_size, "label {label}");
}

#[test]
fn typed_line_larger_than_keys() {
    let mut f = field(false, false, "User name");
    f.text = "View-Pass".into();
    let (_, texts) = painted(&f);
    let line = texts.iter().find(|(s, _)| s == "View-Pass").expect("typed line").1;
    let key = texts.iter().find(|(s, _)| s == "a").expect("key a").1;
    assert!(line >= 1.4 * key, "line {line} vs key {key}");
}

#[test]
fn only_focused_key_white_and_scaled() {
    let t = Theme::placeholder();
    let (plates, _) = painted(&field(false, false, "User name"));
    let keys: Vec<_> = plates.iter().filter(|(r, _)| r.width() < 700.0).collect();
    let white: Vec<_> = keys.iter().filter(|(_, c)| c.r() > 0x80).collect();
    assert_eq!(white.len(), 1, "only the focused key is white");
    let rest = keys.iter().find(|(_, c)| c.r() <= 0x80).unwrap().0.height();
    assert!(
        white[0].0.height() >= 1.15 * rest && white[0].0.height() / rest != t.pill_focus_scale,
        "{} vs {rest}",
        white[0].0.height()
    );
}

/// A Jellyfin server on `ip` with one public user who has a password, Quick Connect on (all four sign-in buttons).
fn picker_server(ip: std::net::Ipv4Addr) -> String {
    use std::io::{Read, Write};
    let l = std::net::TcpListener::bind((ip, 0)).unwrap();
    let base = format!("http://{ip}:{}", l.local_addr().unwrap().port());
    std::thread::spawn(move || {
        for mut s in l.incoming().flatten() {
            let mut buf = [0u8; 4096];
            let _ = s.set_read_timeout(Some(std::time::Duration::from_secs(2)));
            let n = s.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).into_owned();
            let path = req.split_whitespace().nth(1).unwrap_or("");
            let (code, body) = if path.starts_with("/System/Info/Public") {
                ("200 OK", r#"{"ServerName":"Kb","Version":"10.10.7","Id":"kb-picker"}"#)
            } else if path.starts_with("/Users/Public") {
                ("200 OK", r#"[{"Name":"tester","Id":"u1","HasPassword":true}]"#)
            } else if path.starts_with("/QuickConnect/Enabled") {
                ("200 OK", "true")
            } else if path.starts_with("/Branding/Configuration") {
                ("200 OK", "{}")
            } else {
                ("404 Not Found", "")
            };
            let _ = write!(
                s,
                "HTTP/1.1 {code}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    base
}

/// One frame; the painted text with its on-screen box (its clip applied: an empty box = not shown).
fn frame_texts(app: &mut App, ctx: &egui::Context, keys: &[Key]) -> Vec<(String, egui::Rect)> {
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1920.0, 1080.0));
    let out = ctx.run(egui::RawInput { screen_rect: Some(rect), ..Default::default() }, |ctx| {
        app.frame(ctx, &mut NullPlayer::default(), keys)
    });
    out.shapes
        .into_iter()
        .filter_map(|c| match &c.shape {
            egui::Shape::Text(tx) => {
                let r = tx.visual_bounding_rect().intersect(c.clip_rect);
                Some((tx.galley.text().to_string(), r))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn signin_buttons_stay_above_the_open_keyboard() {
    use jf_core::settings::{ServerEntry, Settings};
    let base = picker_server(std::net::Ipv4Addr::new(127, 0, 0, 41));
    let store = Arc::new(MemStorage::default());
    let mut s = Settings::load(store.as_ref());
    s.remember(ServerEntry { id: "kb-picker".into(), name: "Kb".into(), address: base, session: None });
    s.save(store.as_ref()).unwrap();
    let mut app = App::new(store, AppOptions { device_name: "kb-test".into(), ..Default::default() });
    let ctx = egui::Context::default();
    ctx.set_fonts(Theme::fonts());
    // "Who's watching?" with the tester tile and all four buttons
    let start = std::time::Instant::now();
    let has = |texts: &[(String, egui::Rect)], s: &str| texts.iter().any(|(t, _)| t == s);
    loop {
        let texts = frame_texts(&mut app, &ctx, &[]);
        if has(&texts, "tester") && has(&texts, "Use Quick Connect") {
            break;
        }
        assert!(start.elapsed().as_secs() < 10, "picker never loaded: {}", app.screen_name());
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    // Other user… opens the keyboard on the user name
    frame_texts(&mut app, &ctx, &[Key::Right]);
    frame_texts(&mut app, &ctx, &[Key::Enter]);
    let mut texts = vec![];
    for _ in 0..4 {
        texts = frame_texts(&mut app, &ctx, &[]);
    }
    let kb = panel_rect(
        &Theme::placeholder(),
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1920.0, 1080.0)),
        layout(false, false).len(),
    );
    assert!(has(&texts, "space"), "the keyboard is up");
    for b in ["Sign in", "Use Quick Connect", "Forgot password?", "Change server", "Remember me"] {
        let r = texts.iter().find(|(t, _)| t == b).map(|(_, r)| *r).unwrap_or_else(|| panic!("{b} not painted"));
        assert!(r.is_positive() && r.bottom() <= kb.top(), "{b} at {r:?} is not above the keyboard (top {})", kb.top());
    }
}
