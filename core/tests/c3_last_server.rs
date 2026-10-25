//! C3 discovery (BRIEF-discovery item 2): the last server opens straight away (sign-in or Home, no Connect screen);
//! when it doesn't answer, discovery runs and a server that moved to a new address is listed focused on Connect. The
//! saved session (token) goes to the new address only after the user presses it (a discovery reply's Id is public).
//! Fake servers only: a UDP responder on 127.0.0.x (probe port injected) and a tiny HTTP server that answers every
//! request with the public info.
use jf_core::api::Session;
use jf_core::app::{App, AppOptions};
use jf_core::discovery::{set_probe_port, DISCOVERY_MESSAGE};
use jf_core::platform::{Key, NullPlayer, Storage};
use jf_core::settings::{MemStorage, ServerEntry, Settings};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener, UdpSocket};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const ID: &str = "c3-moved-server";

fn probe_port() -> u16 {
    41000 + (std::process::id() % 5000) as u16
}

fn frame(app: &mut App, ctx: &egui::Context, player: &mut NullPlayer) {
    frame_keys(app, ctx, player, &[]);
}

fn frame_keys(app: &mut App, ctx: &egui::Context, player: &mut NullPlayer, keys: &[Key]) {
    let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1920.0, 1080.0));
    let _ =
        ctx.run(egui::RawInput { screen_rect: Some(rect), ..Default::default() }, |ctx| app.frame(ctx, player, keys));
}

/// Frames for `d` (lets a search finish and its result land).
fn run_for(app: &mut App, ctx: &egui::Context, d: Duration) {
    run_until(app, ctx, d, |_| false);
}

/// Runs frames until `done` or `cap`.
fn run_until(app: &mut App, ctx: &egui::Context, cap: Duration, mut done: impl FnMut(&App) -> bool) -> bool {
    let mut player = NullPlayer::default();
    let start = Instant::now();
    while !done(app) && start.elapsed() < cap {
        frame(app, ctx, &mut player);
        std::thread::sleep(Duration::from_millis(10));
    }
    done(app)
}

type Log = Arc<Mutex<Vec<String>>>;

/// An HTTP server on `ip` answering every request with the public info of `ID`; returns its base address and the
/// log of requests it got (request line + headers).
fn http_server(ip: Ipv4Addr) -> (String, Log) {
    let l = TcpListener::bind((ip, 0)).unwrap();
    let base = format!("http://{ip}:{}", l.local_addr().unwrap().port());
    let log: Log = Arc::default();
    let seen = log.clone();
    std::thread::spawn(move || {
        for mut s in l.incoming().flatten() {
            let mut buf = [0u8; 4096];
            let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
            let n = s.read(&mut buf).unwrap_or(0);
            seen.lock().unwrap().push(String::from_utf8_lossy(&buf[..n]).into_owned());
            let body = format!(r#"{{"ServerName":"Moved","Version":"10.10.7","Id":"{ID}"}}"#);
            let _ = write!(
                s,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    (base, log)
}

fn with_token(log: &Log) -> usize {
    log.lock().unwrap().iter().filter(|r| r.contains("Token=")).count()
}

/// A discovery responder (unicast only: bound to one address) saying server `ID` now lives at `address`.
fn responder(ip: Ipv4Addr, address: String) {
    let sock = UdpSocket::bind((ip, probe_port())).expect("bind responder");
    std::thread::spawn(move || {
        let mut buf = [0u8; 256];
        while let Ok((n, from)) = sock.recv_from(&mut buf) {
            if &buf[..n] == DISCOVERY_MESSAGE {
                let r = format!(r#"{{"Address":"{address}","Id":"{ID}","Name":"Moved"}}"#);
                let _ = sock.send_to(r.as_bytes(), from);
            }
        }
    });
}

fn app_with(address: &str, session: Option<Session>) -> (App, Arc<MemStorage>) {
    set_probe_port(probe_port());
    let store = Arc::new(MemStorage::default());
    let mut s = Settings::load(store.as_ref());
    s.remember(ServerEntry { id: ID.into(), name: "Moved".into(), address: address.into(), session });
    s.save(store.as_ref()).unwrap();
    let app = App::new(store.clone(), AppOptions { device_name: "c3-test".into(), ..Default::default() });
    (app, store)
}

fn saved_address(store: &MemStorage) -> String {
    Settings::load(store as &dyn Storage).last().map(|e| e.address.clone()).unwrap_or_default()
}

#[test]
fn last_server_opens_sign_in_without_the_connect_screen() {
    let (mut app, _) = app_with("http://127.0.0.11:9", None);
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, &mut NullPlayer::default());
    assert_eq!(app.screen_name(), "signin");
}

#[test]
fn last_server_with_a_session_opens_home() {
    let session = Session { user_id: "u".into(), user_name: "tester".into(), token: "t".into() };
    let (mut app, _) = app_with("http://127.0.0.12:9", Some(session));
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, &mut NullPlayer::default());
    assert_eq!(app.screen_name(), "home");
}

#[test]
fn silent_last_server_falls_back_to_discovery() {
    // nothing listens on 127.0.0.13:9 and no responder: sign-in gives way to the Connect screen (discovery)
    let (mut app, _) = app_with("http://127.0.0.13:9", None);
    let ctx = egui::Context::default();
    assert!(
        run_until(&mut app, &ctx, Duration::from_secs(10), |a| a.screen_name() == "connect"),
        "{}",
        app.screen_name()
    );
}

#[test]
fn moved_server_is_offered_and_one_press_opens_sign_in_there() {
    let (new, log) = http_server(Ipv4Addr::new(127, 0, 0, 14));
    responder(Ipv4Addr::new(127, 0, 0, 14), new.clone());
    let (mut app, store) = app_with("http://127.0.0.14:9", None);
    let ctx = egui::Context::default();
    assert!(
        run_until(&mut app, &ctx, Duration::from_secs(10), |a| a.screen_name() == "connect"),
        "{}",
        app.screen_name()
    );
    run_for(&mut app, &ctx, Duration::from_secs(2));
    assert_eq!(app.screen_name(), "connect", "no automatic connect");
    assert!(log.lock().unwrap().is_empty(), "nothing sent to the new address before the press");
    assert_eq!(saved_address(&store), "http://127.0.0.14:9");
    // the found server is focused: OK continues there
    frame_keys(&mut app, &ctx, &mut NullPlayer::default(), &[Key::Enter]);
    assert!(
        run_until(&mut app, &ctx, Duration::from_secs(10), |a| a.screen_name() == "signin"),
        "{}",
        app.screen_name()
    );
    assert_eq!(saved_address(&store), new);
    assert_eq!(with_token(&log), 0);
}

#[test]
fn moved_server_gets_the_token_only_after_the_press() {
    let (new, log) = http_server(Ipv4Addr::new(127, 0, 0, 15));
    responder(Ipv4Addr::new(127, 0, 0, 15), new.clone());
    let session = Session { user_id: "u".into(), user_name: "tester".into(), token: "t".into() };
    let (mut app, store) = app_with("http://127.0.0.15:9", Some(session));
    let ctx = egui::Context::default();
    // Home can't verify at the saved address → the search finds the server elsewhere → Connect, server focused
    assert!(
        run_until(&mut app, &ctx, Duration::from_secs(10), |a| a.screen_name() == "connect"),
        "{}",
        app.screen_name()
    );
    run_for(&mut app, &ctx, Duration::from_secs(2));
    assert_eq!(app.screen_name(), "connect");
    assert!(log.lock().unwrap().is_empty(), "nothing sent to the new address before the press");
    assert_eq!(saved_address(&store), "http://127.0.0.15:9");
    frame_keys(&mut app, &ctx, &mut NullPlayer::default(), &[Key::Enter]);
    assert!(run_until(&mut app, &ctx, Duration::from_secs(10), |a| a.screen_name() == "home"), "{}", app.screen_name());
    assert_eq!(saved_address(&store), new);
    let s = Settings::load(store.as_ref() as &dyn Storage);
    assert_eq!(s.last().unwrap().session.as_ref().map(|s| s.token.as_str()), Some("t"), "session kept");
    // the press first asked for the public info without a token; the token follows only after the Id matched
    let reqs = log.lock().unwrap().clone();
    let first = reqs.first().expect("a request after the press");
    assert!(first.contains("/System/Info/Public") && !first.contains("Token="), "{first}");
}
