//! FIX-4 F4-PLAYLISTS (FRICTION-5 #5): playlists reachable from a poster; no duplicate tiles.
//! PL2–PL4 drive the real App / Client against a stub server (no jf-test needed).
use jf_core::api::{Client, DeviceInfo, Item, ItemKind, Session};
use jf_core::app::{App, AppOptions, Cx, Screen};
use jf_core::platform::{Key, NullPlayer};
use jf_core::settings::{MemStorage, ServerEntry, Settings};
use jf_core::ui::card_menu::{entries, MenuAct};
use jf_core::ui::{card_menu, playlists};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

type Log = Arc<Mutex<Vec<String>>>;

fn film() -> Item {
    Item { id: "f1".into(), name: "Amber Tide".into(), kind: ItemKind::Movie, ..Default::default() }
}

/// Playlists "Weekend" (a) and "Rainy" (b), with "Weekend" repeated — what a doubled server reply looks like.
const DOUBLED: &str = r#"{"Items":[
 {"Id":"a","Name":"Weekend","Type":"Playlist"},
 {"Id":"b","Name":"Rainy","Type":"Playlist"},
 {"Id":"a","Name":"Weekend","Type":"Playlist"}],"TotalRecordCount":3}"#;

fn route_doubled(path: &str) -> String {
    if path.contains("IncludeItemTypes=Playlist") {
        DOUBLED.into()
    } else if path.starts_with("/Playlists/") && path.contains("/Users/") {
        r#"{"CanEdit":true}"#.into()
    } else {
        r#"{"Items":[],"TotalRecordCount":0}"#.into()
    }
}

fn route_empty(path: &str) -> String {
    let _ = path;
    r#"{"Items":[],"TotalRecordCount":0}"#.into()
}

/// Tiny HTTP stub: answers each request by path via `route`, logs request lines.
fn stub(route: fn(&str) -> String) -> (String, Log) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let log: Log = Arc::default();
    let seen = log.clone();
    std::thread::spawn(move || {
        for conn in l.incoming() {
            let Ok(mut conn) = conn else { continue };
            let mut r = BufReader::new(conn.try_clone().unwrap());
            let mut line = String::new();
            if r.read_line(&mut line).is_err() {
                continue;
            }
            let mut len = 0usize;
            loop {
                let mut h = String::new();
                if r.read_line(&mut h).is_err() || h == "\r\n" || h.is_empty() {
                    break;
                }
                if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap_or(0);
                }
            }
            let mut body = vec![0u8; len];
            let _ = std::io::Read::read_exact(&mut r, &mut body);
            let path = line.split_whitespace().nth(1).unwrap_or("").to_string();
            seen.lock().unwrap().push(path.clone());
            let json = route(&path);
            let _ = write!(
                conn,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{json}",
                json.len()
            );
        }
    });
    (base, log)
}

fn session() -> Session {
    Session { user_id: "u1".into(), user_name: "tester".into(), token: "tok".into() }
}

fn client(base: &str) -> Client {
    Client::with_session(base, DeviceInfo { name: "t".into(), id: "d".into(), version: "0".into() }, session())
}

fn app_on(base: &str) -> App {
    let store = Arc::new(MemStorage::default());
    let mut s = Settings::load(store.as_ref());
    s.remember(ServerEntry { id: "s".into(), name: "S".into(), address: base.into(), session: Some(session()) });
    s.save(store.as_ref()).unwrap();
    App::new(store, AppOptions { device_name: "f4".into(), ..Default::default() })
}

/// One frame; returns every string drawn.
fn frame(app: &mut App, ctx: &egui::Context, keys: &[Key]) -> Vec<String> {
    let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1920.0, 1080.0));
    let mut player = NullPlayer::default();
    let out = ctx
        .run(egui::RawInput { screen_rect: Some(rect), ..Default::default() }, |ctx| app.frame(ctx, &mut player, keys));
    out.shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::epaint::Shape::Text(t) => Some(t.galley.text().to_string()),
            _ => None,
        })
        .collect()
}

/// Frames until `done(drawn strings)` or `cap`; returns the last frame's strings.
fn run_until(app: &mut App, ctx: &egui::Context, cap: Duration, done: impl Fn(&App, &[String]) -> bool) -> Vec<String> {
    let start = Instant::now();
    loop {
        let t = frame(app, ctx, &[]);
        if done(app, &t) || start.elapsed() > cap {
            return t;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn count(t: &[String], what: &str) -> usize {
    t.iter().filter(|s| s.trim() == what).count()
}

fn asked_for_playlists(log: &Log) -> bool {
    log.lock().unwrap().iter().any(|p| p.contains("IncludeItemTypes=Playlist"))
}

/// A throwaway screen that, on its first frame, runs the card menu's act for a film (the poster Info menu's choice).
struct Chooser(Option<MenuAct>);
impl Screen for Chooser {
    fn name(&self) -> &'static str {
        "chooser"
    }
    fn ui(&mut self, _ui: &mut egui::Ui, cx: &mut Cx) {
        if let Some(act) = self.0.take() {
            card_menu::run(cx, &film(), act, None);
        }
    }
}

/// PL1: "Add to…" sits after Favourite and before Poster Flow; Poster Flow stays last.
#[test]
fn info_menu_has_add_to_before_poster_flow() {
    for admin in [false, true] {
        let e = entries(&film(), admin);
        let pos = |a: MenuAct| e.iter().position(|(x, _)| *x == a).unwrap();
        assert_eq!(e[pos(MenuAct::AddTo)].1, "Add to…");
        assert!(pos(MenuAct::ToggleFavourite) < pos(MenuAct::AddTo));
        assert!(pos(MenuAct::AddTo) < pos(MenuAct::PosterFlow));
        assert_eq!(pos(MenuAct::PosterFlow), e.len() - 1);
    }
    // an episode too; a show cannot be added to a playlist from here
    let ep = Item { kind: ItemKind::Episode, ..Default::default() };
    assert!(entries(&ep, false).iter().any(|(a, _)| *a == MenuAct::AddTo));
    let show = Item { kind: ItemKind::Series, ..Default::default() };
    assert!(!entries(&show, false).iter().any(|(a, _)| *a == MenuAct::AddTo));
}

/// PL2: choosing "Add to…" runs through `card_menu::run`, lands on the film's page, and that page opens its Add-to
/// menu on its own (it asks the server for the user's playlists); a plain Details page does not.
#[test]
fn info_add_to_opens_page_with_menu() {
    let (base, log) = stub(route_doubled);
    let mut app = app_on(&base);
    let ctx = egui::Context::default();
    app.open_screen(Box::new(Chooser(Some(MenuAct::AddTo))));
    run_until(&mut app, &ctx, Duration::from_secs(10), |a, _| a.screen_name() == "movie" && asked_for_playlists(&log));
    assert_eq!(app.screen_name(), "movie", "Add to… opens the film's page");
    assert!(asked_for_playlists(&log), "and its Add-to menu is open (it loaded the playlists)");

    // control: Details opens the same page without the menu
    let (base, log) = stub(route_doubled);
    let mut app = app_on(&base);
    app.open_screen(Box::new(Chooser(Some(MenuAct::Details))));
    run_until(&mut app, &ctx, Duration::from_millis(1500), |_, _| false);
    assert_eq!(app.screen_name(), "movie");
    assert!(!asked_for_playlists(&log), "a normal page opens no menu");
}

/// PL3: the empty Playlists page, drawn, names "Add to…".
#[test]
fn empty_playlists_names_add_to() {
    let (base, _log) = stub(route_empty);
    let mut app = app_on(&base);
    let ctx = egui::Context::default();
    app.open_screen(playlists::list());
    let t = run_until(&mut app, &ctx, Duration::from_secs(10), |_, t| t.iter().any(|s| s.contains("No playlists yet")));
    let line =
        t.iter().find(|s| s.contains("No playlists yet")).unwrap_or_else(|| panic!("empty text not drawn: {t:?}"));
    assert!(line.contains("Add to…"), "{line}");
}

/// PL4: a doubled server reply shows each playlist id once — on the Playlists page and in the Add-to picker's list.
#[test]
fn playlist_list_dedupes_by_id() {
    let (base, _log) = stub(route_doubled);
    let c = client(&base);
    let ids = |v: Vec<Item>| v.into_iter().map(|i| i.id).collect::<Vec<_>>();
    assert_eq!(ids(c.playlists().unwrap()), ["a", "b"]);
    assert_eq!(ids(c.editable_playlists().unwrap()), ["a", "b"]);

    let mut app = app_on(&base);
    let ctx = egui::Context::default();
    app.open_screen(playlists::list());
    let t = run_until(&mut app, &ctx, Duration::from_secs(10), |_, t| count(t, "Rainy") > 0);
    assert_eq!(count(&t, "Weekend"), 1, "{t:?}");
    assert_eq!(count(&t, "Rainy"), 1, "{t:?}");
}

/// PL4: leaving the page and coming back (a reload on top of the first load) still shows each id once.
#[test]
fn reload_race_no_duplicate() {
    let (base, _log) = stub(route_doubled);
    let mut app = app_on(&base);
    let ctx = egui::Context::default();
    app.open_screen(playlists::list());
    run_until(&mut app, &ctx, Duration::from_secs(10), |_, t| count(t, "Rainy") > 0);
    app.open_screen(Box::new(Chooser(None)));
    frame(&mut app, &ctx, &[]);
    frame(&mut app, &ctx, &[Key::Back]);
    assert_eq!(app.screen_name(), "playlists");
    // reload mid-flight: keep drawing, never more than one tile of each id at any frame
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(1500) {
        let t = frame(&mut app, &ctx, &[]);
        assert!(count(&t, "Weekend") <= 1 && count(&t, "Rainy") <= 1, "{t:?}");
        std::thread::sleep(Duration::from_millis(10));
    }
}
