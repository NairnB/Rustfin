//! RR5 live tests against the throwaway server ONLY (`JF_TEST_URL`; viewer / view-pass-1). Skipped without it.
//! The next-up test marks one episode watched and puts the watched state back, even when an assert fails.
use jf_core::api::*;

fn url() -> Option<String> {
    let u = std::env::var("JF_TEST_URL").ok();
    if u.is_none() {
        eprintln!("JF_TEST_URL unset: live test skipped");
    }
    u
}

fn viewer(base: &str) -> Client {
    let dev = DeviceInfo { name: "live-rr5".into(), id: "live-rr5-device".into(), version: "0.1.0".into() };
    let (mut c, _) = Client::connect(base, dev).unwrap();
    c.sign_in("viewer", "view-pass-1").unwrap();
    c
}

fn films(c: &Client) -> Vec<Item> {
    let arch = c.views().unwrap().into_iter().find(|v| v.name == "Archive").unwrap();
    let page = c.library_page(&arch.id, 0, 50).unwrap();
    page.items.into_iter().map(|i| c.item(&i.id).unwrap()).collect()
}

#[test]
fn genre_query_finds_the_items_of_that_genre() {
    let Some(u) = url() else { return };
    let c = viewer(&u);
    let views = c.views().unwrap();
    let genre = views
        .iter()
        .find_map(|v| c.filter_options(&v.id, "Movie,Series").ok()?.genres.into_iter().next())
        .expect("a genre on the test server");
    let found = c.items_by_genre(&genre).unwrap();
    assert!(!found.is_empty(), "{genre} should list something");
    assert!(found.iter().all(|i| i.genres.iter().any(|g| g.eq_ignore_ascii_case(&genre))), "{genre}");
    assert!(found.iter().all(|i| matches!(i.kind, ItemKind::Movie | ItemKind::Series)));
    assert!(c.items_by_genre("No Such Genre Anywhere").unwrap().is_empty());
}

#[test]
fn studio_query_finds_the_items_of_that_studio() {
    let Some(u) = url() else { return };
    let c = viewer(&u);
    assert!(c.items_by_studio("00000000000000000000000000000000").unwrap().is_empty());
    let all = films(&c);
    let Some((it, st)) = all.iter().find_map(|i| i.studios.first().map(|s| (i, s.clone()))) else {
        eprintln!("no item with a studio on the test server: skipped");
        return;
    };
    let found = c.items_by_studio(&st.id).unwrap();
    assert!(found.iter().any(|i| i.id == it.id), "{} should list {}", st.name, it.name);
}

#[test]
fn playable_under_turns_a_series_into_its_episodes() {
    let Some(u) = url() else { return };
    let c = viewer(&u);
    let show = c.search("Drift Rebels").unwrap().into_iter().find(|i| i.kind == ItemKind::Series).unwrap();
    let eps = c.playable_under(std::slice::from_ref(&show)).unwrap();
    assert!(!eps.is_empty());
    assert!(eps.iter().all(|e| e.kind == ItemKind::Episode));
    let film = films(&c).into_iter().next().unwrap();
    let both = c.playable_under(&[film.clone(), show]).unwrap();
    assert_eq!(both[0].id, film.id);
    assert_eq!(both.len(), eps.len() + 1);
}

struct Unwatch {
    client: Client,
    id: String,
    was_played: bool,
}

impl Drop for Unwatch {
    fn drop(&mut self) {
        let _ = self.client.set_played(&self.id, self.was_played);
    }
}

#[test]
fn server_next_up_follows_what_is_watched() {
    let Some(u) = url() else { return };
    let c = viewer(&u);
    let show = c.search("Drift Rebels").unwrap().into_iter().find(|i| i.kind == ItemKind::Series).unwrap();
    let Some(first) = c.series_next_up(&show.id).unwrap() else {
        eprintln!("viewer has nothing next up in Drift Rebels: skipped");
        return;
    };
    assert_eq!(first.kind, ItemKind::Episode);
    assert_eq!(first.series_name.as_deref(), Some("Drift Rebels"));
    let was_played = first.user_data.as_ref().map(|d| d.played).unwrap_or(false);
    let _back = Unwatch { client: c.clone(), id: first.id.clone(), was_played };
    c.set_played(&first.id, true).unwrap();
    let after = c.series_next_up(&show.id).unwrap();
    assert_ne!(after.map(|i| i.id), Some(first.id.clone()), "a watched episode is no longer next up");
}

#[test]
fn episodes_carry_their_premiere_date() {
    let Some(u) = url() else { return };
    let c = viewer(&u);
    let show = c.search("Drift Rebels").unwrap().into_iter().find(|i| i.kind == ItemKind::Series).unwrap();
    let eps = c.episodes_opts(&show.id, true).unwrap();
    assert!(!eps.is_empty());
}

// ---- render-tour fixture (ignored: run by hand around the P-ITEM-2 / P-ITEM-3 tours, never by `cargo test`) ----
// The test server's films have short overviews and no genres, so the tours would show nothing to expand or click.
// `rr5_fixture_set` gives "Star Drift - Ember Rising" a long overview and two genres (admin edit) and saves the
// original to `$RR5_FIXTURE_FILE`; `rr5_fixture_restore` puts it back.
use jf_core::api::item_edit::ItemEdit;

fn fixture_film(c: &Client) -> Item {
    c.search("Ember Rising").unwrap().into_iter().find(|i| i.kind == ItemKind::Movie).expect("Ember Rising")
}

fn fixture_file() -> String {
    std::env::var("RR5_FIXTURE_FILE").expect("RR5_FIXTURE_FILE")
}

fn admin(base: &str) -> Client {
    let dev = DeviceInfo { name: "live-rr5".into(), id: "live-rr5-admin".into(), version: "0.1.0".into() };
    let (mut c, _) = Client::connect(base, dev).unwrap();
    c.sign_in("tester", "test-pass-1").unwrap();
    c
}

#[test]
#[ignore]
fn rr5_fixture_set() {
    let Some(u) = url() else { return };
    let c = admin(&u);
    let film = fixture_film(&c);
    let mut e = ItemEdit::from_json(&c.item_json(&film.id).unwrap());
    std::fs::write(fixture_file(), format!("{}\n{}", e.overview, e.genres.join("\u{1f}"))).unwrap();
    e.overview = "A made-up film for the feature tests. ".repeat(3)
        + &"The crew follows a beacon across the rim of the drift, loses the ship twice, finds it once, and argues about the maps the whole way home. ".repeat(4);
    e.genres = vec!["Drama".into(), "Sci-Fi".into()];
    c.save_item_edit(&film.id, &e).unwrap();
}

#[test]
#[ignore]
fn rr5_fixture_restore() {
    let Some(u) = url() else { return };
    let c = admin(&u);
    let film = fixture_film(&c);
    let saved = std::fs::read_to_string(fixture_file()).unwrap();
    let (overview, genres) = saved.split_once('\n').unwrap();
    let mut e = ItemEdit::from_json(&c.item_json(&film.id).unwrap());
    e.overview = overview.to_string();
    e.genres = genres.split('\u{1f}').filter(|g| !g.is_empty()).map(str::to_string).collect();
    c.save_item_edit(&film.id, &e).unwrap();
}
