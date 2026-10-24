//! Poster picker live tests (PP) against the throwaway server ONLY (`JF_TEST_URL`; admin tester / test-pass-1,
//! non-admin viewer / view-pass-1). Skipped without JF_TEST_URL. The admin test changes one film's Primary and
//! always puts it back; the viewer test writes only that viewer's DisplayPreferences pref and clears it.
use jf_core::api::poster::{resolve_poster, run_apply, undo_plan, PosterChoice, PosterStep, POSTER_PREFS_ID};
use jf_core::api::*;

fn url() -> Option<String> {
    let u = std::env::var("JF_TEST_URL").ok();
    if u.is_none() {
        eprintln!("JF_TEST_URL unset: live test skipped");
    }
    u
}

fn signed(base: &str, user: &str, pass: &str) -> Client {
    let dev = DeviceInfo { name: "live-poster".into(), id: "live-poster-device".into(), version: "0.1.0".into() };
    let (mut c, _) = Client::connect(base, dev).unwrap();
    c.sign_in(user, pass).unwrap();
    c
}

fn archive_film(c: &Client) -> Item {
    let arch = c.views().unwrap().into_iter().find(|v| v.name == "Archive").unwrap();
    c.library_page(&arch.id, 0, 1).unwrap().items.remove(0)
}

/// A generated 2×3 poster PNG, for the offline fallback and undo.
fn png() -> Vec<u8> {
    let img = image::RgbaImage::from_fn(200, 300, |x, y| image::Rgba([(x % 256) as u8, (y % 256) as u8, 140, 255]));
    let mut out = std::io::Cursor::new(vec![]);
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

fn hash(bytes: &[u8]) -> String {
    // A short FNV-1a, good enough to prove the bytes came back identical.
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

/// Restores the film's Primary to the saved bytes, also when an assertion fails.
struct Restore {
    client: Client,
    id: String,
    bytes: Vec<u8>,
}

impl Drop for Restore {
    fn drop(&mut self) {
        let _ = self.client.set_item_image(&self.id, &self.bytes);
    }
}

#[test]
fn admin_changes_the_primary_and_undo_restores_the_bytes() {
    let Some(u) = url() else { return };
    let admin = signed(&u, "tester", "test-pass-1");
    let film = archive_film(&admin);
    let before = admin.item_image_bytes(&film.id).unwrap();
    let before_tag = admin.item(&film.id).unwrap().image_tags.get("Primary").cloned();
    let _restore = Restore { client: admin.clone(), id: film.id.clone(), bytes: before.clone() };

    // Offline the server returns no remote candidates: upload a generated 2×3 PNG instead.
    let cands = admin.remote_images(&film.id).unwrap();
    if let Some(url) = cands.iter().find_map(|c| c.full_url().map(str::to_string)) {
        admin.download_remote_image(&film.id, &url).unwrap();
    } else {
        admin.set_item_image(&film.id, &png()).unwrap();
    }

    let after = admin.item(&film.id).unwrap();
    let after_tag = after.image_tags.get("Primary").cloned();
    assert_ne!(before_tag, after_tag, "applying a new poster changes the ImageTag");

    // Undo: re-upload the saved previous bytes.
    admin.set_item_image(&film.id, &before).unwrap();
    let undone = admin.item_image_bytes(&film.id).unwrap();
    assert_eq!(hash(&undone), hash(&before), "undo restores the original bytes");
}

/// Clears the viewer's personal poster pref, also when an assertion fails.
struct ClearPref {
    client: Client,
    dpid: String,
    id: String,
}

impl Drop for ClearPref {
    fn drop(&mut self) {
        let _ = self.client.set_poster_pref(&self.dpid, &self.id, None);
    }
}

#[test]
fn a_viewer_pick_stays_personal_and_never_changes_the_server_image() {
    let Some(u) = url() else { return };
    let admin = signed(&u, "tester", "test-pass-1");
    let viewer = signed(&u, "viewer", "view-pass-1");
    let film = archive_film(&admin);
    let dpid = POSTER_PREFS_ID.to_string();
    let _clear = ClearPref { client: viewer.clone(), dpid: dpid.clone(), id: film.id.clone() };

    let before_tag = admin.item(&film.id).unwrap().image_tags.get("Primary").cloned();

    viewer.set_poster_pref(&dpid, &film.id, Some("https://example.test/poster.jpg")).unwrap();
    let prefs = viewer.display_preferences(&dpid).unwrap();
    assert_eq!(
        resolve_poster(&prefs.custom_prefs, &film.id).as_deref(),
        Some("https://example.test/poster.jpg"),
        "the pick round-trips through the user's DisplayPreferences"
    );

    let after_tag = admin.item(&film.id).unwrap().image_tags.get("Primary").cloned();
    assert_eq!(before_tag, after_tag, "a viewer's pick never changes the shared image");

    // Clearing the pref removes the override.
    viewer.set_poster_pref(&dpid, &film.id, None).unwrap();
    let prefs = viewer.display_preferences(&dpid).unwrap();
    assert_eq!(resolve_poster(&prefs.custom_prefs, &film.id), None);
}

/// Runs one admin apply for real: the plan functions, executed against the server.
fn admin_apply(c: &Client, id: &str, choice: &PosterChoice) -> Option<Vec<u8>> {
    run_apply(true, choice, |s| c.run_poster_step(id, Some(POSTER_PREFS_ID), s), |_, _| Ok(())).unwrap()
}

#[test]
fn an_item_without_a_poster_gets_a_first_poster_and_undo_deletes_it() {
    let Some(u) = url() else { return };
    let admin = signed(&u, "tester", "test-pass-1");
    let film = archive_film(&admin);
    let before = admin.item_image_bytes(&film.id).unwrap();
    let _restore = Restore { client: admin.clone(), id: film.id.clone(), bytes: before.clone() };

    // Make the film poster-less.
    admin.delete_item_image(&film.id).unwrap();
    assert!(admin.item_image_bytes(&film.id).is_err(), "no poster now");

    // Apply a first poster: the backup 404 is "nothing to back up", not a failure.
    let previous = admin_apply(&admin, &film.id, &PosterChoice::OwnImage { bytes: png() });
    assert_eq!(previous, None);
    assert!(admin.item_image_bytes(&film.id).is_ok(), "the first poster is there");

    // Undo for an item that had none = delete the image.
    for step in undo_plan(true, previous.as_deref()) {
        assert_eq!(step, PosterStep::DeleteImage);
        admin.run_poster_step(&film.id, None, &step).unwrap();
    }
    assert!(admin.item_image_bytes(&film.id).is_err(), "undo removed the first poster");
}

mod app_level {
    use super::*;
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

    fn until(
        app: &mut App,
        ctx: &egui::Context,
        player: &mut NullPlayer,
        what: &str,
        mut ok: impl FnMut(&App) -> bool,
    ) {
        let start = Instant::now();
        while !ok(app) {
            assert!(start.elapsed() < Duration::from_secs(25), "timed out waiting for {what}");
            frame(app, ctx, player, &[], vec![]);
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn opts() -> AppOptions {
        AppOptions { device_name: "live-poster-app".into(), ..Default::default() }
    }

    /// Sign in through the real screens; returns once Home is up.
    fn sign_in(app: &mut App, ctx: &egui::Context, player: &mut NullPlayer, url: &str, user: &str, pass: &str) {
        for _ in 0..5 {
            frame(app, ctx, player, &[], vec![]);
        }
        frame(app, ctx, player, &[], vec![egui::Event::Text(url.to_string())]);
        frame(app, ctx, player, &[Key::Enter], vec![]);
        until(app, ctx, player, "signin", |a| a.screen_name() == "signin");
        for _ in 0..5 {
            frame(app, ctx, player, &[], vec![]);
        }
        frame(app, ctx, player, &[], vec![egui::Event::Text(user.to_string())]);
        frame(app, ctx, player, &[Key::Down], vec![]);
        frame(app, ctx, player, &[], vec![egui::Event::Text(pass.to_string())]);
        frame(app, ctx, player, &[Key::Enter], vec![]);
        until(app, ctx, player, "home", |a| a.screen_name() == "home");
    }

    #[test]
    fn a_viewers_pick_is_reloaded_from_the_server_at_sign_in_and_kept_per_user() {
        let Some(u) = url() else { return };
        let admin = signed(&u, "tester", "test-pass-1");
        let viewer = signed(&u, "viewer", "view-pass-1");
        let film = archive_film(&admin);
        let _clear = ClearPref { client: viewer.clone(), dpid: POSTER_PREFS_ID.to_string(), id: film.id.clone() };
        viewer.set_poster_pref(POSTER_PREFS_ID, &film.id, Some("https://example.test/mine.jpg")).unwrap();

        // A brand-new App (empty local settings, as on another device) signs the viewer in.
        let store = Arc::new(MemStorage::default());
        let ctx = egui::Context::default();
        let mut player = NullPlayer::default();
        let mut app = App::new(store.clone(), opts());
        sign_in(&mut app, &ctx, &mut player, &u, "viewer", "view-pass-1");
        until(&mut app, &ctx, &mut player, "the pick from the server", |a| a.poster_override(&film.id).is_some());
        assert_eq!(app.poster_override(&film.id).as_deref(), Some("https://example.test/mine.jpg"));
        // Stored under the viewer's user id only.
        let settings = Settings::load(store.as_ref());
        let viewer_id = viewer.session.as_ref().unwrap().user_id.clone();
        assert_eq!(settings.poster_picks.keys().collect::<Vec<_>>(), vec![&viewer_id]);

        // The admin on a fresh App sees no pick of the viewer's.
        let store2 = Arc::new(MemStorage::default());
        let mut app2 = App::new(store2, opts());
        let ctx2 = egui::Context::default();
        sign_in(&mut app2, &ctx2, &mut player, &u, "tester", "test-pass-1");
        for _ in 0..60 {
            frame(&mut app2, &ctx2, &mut player, &[], vec![]);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(app2.poster_override(&film.id), None, "another user's pick never shows");
    }

    #[test]
    fn a_second_screen_shows_the_new_image_tag_after_an_admin_apply() {
        let Some(u) = url() else { return };
        let admin = signed(&u, "tester", "test-pass-1");
        let film = archive_film(&admin);
        let before = admin.item_image_bytes(&film.id).unwrap();
        let old_tag = admin.item(&film.id).unwrap().image_tags.get("Primary").cloned();
        let _restore = Restore { client: admin.clone(), id: film.id.clone(), bytes: before };

        let store = Arc::new(MemStorage::default());
        let ctx = egui::Context::default();
        let mut player = NullPlayer::default();
        let mut app = App::new(store, opts());
        sign_in(&mut app, &ctx, &mut player, &u, "tester", "test-pass-1");
        let archive = admin.views().unwrap().into_iter().find(|v| v.name == "Archive").unwrap();
        app.open_screen(jf_core::ui::library::screen(archive));
        let id = film.id.clone();
        until(&mut app, &ctx, &mut player, "the library's posters", |a| {
            a.top_poster_tags().iter().any(|(i, _)| *i == id)
        });
        let shown = |a: &App| a.top_poster_tags().into_iter().find(|(i, _)| *i == id).and_then(|(_, t)| t);
        assert_eq!(shown(&app), old_tag);

        // Another screen (the picker, here the item page) changes the shared poster, then Back.
        admin_apply(&admin, &film.id, &PosterChoice::OwnImage { bytes: png() });
        let new_tag = admin.item(&film.id).unwrap().image_tags.get("Primary").cloned();
        assert_ne!(old_tag, new_tag);
        app.open_screen(jf_core::ui::item::screen(film.clone()));
        for _ in 0..5 {
            frame(&mut app, &ctx, &mut player, &[], vec![]);
        }
        app.mark_poster_changed(&film.id);
        frame(&mut app, &ctx, &mut player, &[Key::Back], vec![]);
        until(&mut app, &ctx, &mut player, "the grid to show the new ImageTag", |a| {
            a.screen_name() == "library" && shown(a) == new_tag
        });
    }

    #[test]
    fn home_rows_show_the_new_image_tag_and_an_admin_undo_refreshes_at_once() {
        let Some(u) = url() else { return };
        let admin = signed(&u, "tester", "test-pass-1");
        let store = Arc::new(MemStorage::default());
        let ctx = egui::Context::default();
        let mut player = NullPlayer::default();
        let mut app = App::new(store, opts());
        sign_in(&mut app, &ctx, &mut player, &u, "tester", "test-pass-1");
        // Pick a film that Home really shows.
        until(&mut app, &ctx, &mut player, "Home rows", |a| a.top_poster_tags().iter().any(|(_, t)| t.is_some()));
        let (id, old_tag) = app.top_poster_tags().into_iter().find(|(_, t)| t.is_some()).unwrap();
        let before = admin.item_image_bytes(&id).unwrap();
        let _restore = Restore { client: admin.clone(), id: id.clone(), bytes: before.clone() };
        let shown = |a: &App| a.top_poster_tags().into_iter().find(|(i, _)| *i == id).and_then(|(_, t)| t);

        // Admin apply elsewhere, then another screen on top and Back: Home reloads its rows.
        admin_apply(&admin, &id, &PosterChoice::OwnImage { bytes: png() });
        let new_tag = admin.item(&id).unwrap().image_tags.get("Primary").cloned();
        assert_ne!(new_tag, old_tag);
        let item = admin.item(&id).unwrap();
        app.open_screen(jf_core::ui::item::screen(item));
        for _ in 0..5 {
            frame(&mut app, &ctx, &mut player, &[], vec![]);
        }
        app.mark_poster_changed(&id);
        frame(&mut app, &ctx, &mut player, &[Key::Back], vec![]);
        until(&mut app, &ctx, &mut player, "Home to show the new tag", |a| {
            a.screen_name() == "home" && shown(a) == new_tag
        });

        // Admin Undo from the toast (picker already left): the visible Home refreshes without a resume.
        app.press_poster_undo(&id, true, Some(before));
        let restored = |a: &App| shown(a).is_some() && shown(a) != new_tag;
        until(&mut app, &ctx, &mut player, "Home to refresh after Undo", restored);
        let server_tag = admin.item(&id).unwrap().image_tags.get("Primary").cloned();
        assert_eq!(shown(&app), server_tag);
    }
}
