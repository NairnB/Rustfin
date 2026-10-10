//! G8 live tests: similar items (P-ITEM-14), trailers (P-ITEM-8), extras (P-ITEM-15), media info (P-ITEM-18).
//! Skipped without `JF_TEST_URL`; read-only.
use jf_core::api::*;

fn url() -> Option<String> {
    let u = std::env::var("JF_TEST_URL").ok();
    if u.is_none() {
        eprintln!("JF_TEST_URL unset: live test skipped");
    }
    u
}
fn signed(base: &str) -> Client {
    let dev = DeviceInfo { name: "live-test".into(), id: "live-test-device".into(), version: "0.1.0".into() };
    let (mut c, _) = Client::connect(base, dev).unwrap();
    c.sign_in("tester", "test-pass-1").unwrap();
    c
}
fn a_film(c: &Client) -> Item {
    c.search("Amber").unwrap().into_iter().find(|i| i.kind == ItemKind::Movie).expect("a film")
}

#[test]
fn similar_shows_films_for_an_archive_film() {
    let Some(u) = url() else { return };
    let c = signed(&u);
    let film = a_film(&c);
    let v = c.similar(&film.id, 12).unwrap();
    assert!(!v.is_empty(), "no similar items for {}", film.name);
    assert!(v.len() <= 12);
    assert!(v.iter().all(|i| i.id != film.id));
}

#[test]
fn trailers_and_extras_calls_work_and_may_be_empty() {
    let Some(u) = url() else { return };
    let c = signed(&u);
    let film = a_film(&c);
    c.local_trailers(&film.id).unwrap();
    c.special_features(&film.id).unwrap();
}

#[test]
fn media_info_has_the_film_source() {
    let Some(u) = url() else { return };
    let c = signed(&u);
    let film = a_film(&c);
    let v = c.media_info(&film.id).unwrap();
    assert!(!v.is_empty());
    assert!(v.iter().all(|s| !s.id.is_empty()));
}
