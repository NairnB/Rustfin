//! S1: sign-in flow end-to-end against the throwaway server (JF_TEST_URL), driving the real App with no window.
use jf_core::app::{App, AppOptions};
use jf_core::platform::{Key, NullPlayer};
use jf_core::settings::{MemStorage, Settings};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn frame(app: &mut App, ctx: &egui::Context, player: &mut NullPlayer, keys: &[Key], events: Vec<egui::Event>) {
    let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1920.0, 1080.0));
    let _ = ctx.run(egui::RawInput { screen_rect: Some(rect), events, ..Default::default() }, |ctx| {
        app.frame(ctx, player, keys)
    });
}

fn wait_screen(app: &mut App, ctx: &egui::Context, player: &mut NullPlayer, name: &str, cap: Duration) -> bool {
    let start = Instant::now();
    while app.screen_name() != name && start.elapsed() < cap {
        frame(app, ctx, player, &[], vec![]);
        std::thread::sleep(Duration::from_millis(10));
    }
    app.screen_name() == name
}

#[test]
fn sign_in_flow_remembers_session() {
    let Ok(url) = std::env::var("JF_TEST_URL") else {
        eprintln!("JF_TEST_URL unset: skipped");
        return;
    };
    let store = Arc::new(MemStorage::default());
    let ctx = egui::Context::default();
    let mut player = NullPlayer::default();
    let mut app = App::new(
        store.clone(),
        AppOptions { default_server: None, device_name: "flow-test".into(), start_screen: None, desktop: false },
    );

    // Render the connect screen and let the address field take focus (before discovery finishes).
    for _ in 0..5 {
        frame(&mut app, &ctx, &mut player, &[], vec![]);
    }
    assert_eq!(app.screen_name(), "connect");

    // Type the server address and connect.
    frame(&mut app, &ctx, &mut player, &[], vec![egui::Event::Text(url.clone())]);
    frame(&mut app, &ctx, &mut player, &[Key::Enter], vec![]);
    assert!(wait_screen(&mut app, &ctx, &mut player, "signin", Duration::from_secs(20)), "never reached signin");

    // Let the sign-in screen render and the username field take focus.
    for _ in 0..5 {
        frame(&mut app, &ctx, &mut player, &[], vec![]);
    }

    // Username, Down to password, password, Enter to sign in.
    frame(&mut app, &ctx, &mut player, &[], vec![egui::Event::Text("tester".into())]);
    frame(&mut app, &ctx, &mut player, &[Key::Down], vec![]);
    frame(&mut app, &ctx, &mut player, &[], vec![egui::Event::Text("test-pass-1".into())]);
    frame(&mut app, &ctx, &mut player, &[Key::Enter], vec![]);
    assert!(wait_screen(&mut app, &ctx, &mut player, "home", Duration::from_secs(20)), "never reached home");

    let settings = Settings::load(store.as_ref());
    let entry = settings.last().expect("server entry stored");
    let session = entry.session.as_ref().expect("session stored");
    assert_eq!(session.user_name, "tester");
    assert!(entry.address.ends_with(":8096"), "{}", entry.address);
}
