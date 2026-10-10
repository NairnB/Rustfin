//! G3 live tests (favourites): favourite/unfavourite round trip as the non-admin "viewer" on an Archive
//! film, then off again. Against the throwaway server only; skipped without `JF_TEST_URL`.
use jf_core::api::*;

fn url() -> Option<String> {
    let u = std::env::var("JF_TEST_URL").ok();
    if u.is_none() {
        eprintln!("JF_TEST_URL unset: live test skipped");
    }
    u
}
fn dev() -> DeviceInfo {
    DeviceInfo { name: "live-g3".into(), id: "live-g3-device".into(), version: "0.1.0".into() }
}

fn viewer(base: &str) -> Client {
    let (mut c, _) = Client::connect(base, dev()).unwrap();
    c.sign_in("viewer", "view-pass-1").unwrap();
    c
}

fn archive_film(c: &Client) -> Item {
    let arch = c.views().unwrap().into_iter().find(|v| v.name == "Archive").unwrap();
    c.library_page(&arch.id, 0, 1).unwrap().items.into_iter().next().unwrap()
}

#[test]
fn favourite_round_trip() {
    let Some(u) = url() else { return };
    let c = viewer(&u);
    let film = archive_film(&c);
    assert!(!film.name.is_empty());

    // start clean (off)
    c.set_favorite(&film.id, false).unwrap();
    assert!(!c.item(&film.id).unwrap().user_data.unwrap().is_favorite);

    // on → favourite list contains it
    c.set_favorite(&film.id, true).unwrap();
    assert!(c.item(&film.id).unwrap().user_data.unwrap().is_favorite);
    assert!(c.favourites("Movie").unwrap().iter().any(|i| i.id == film.id), "favourite must appear in the list");

    // off again (put it back)
    c.set_favorite(&film.id, false).unwrap();
    assert!(!c.item(&film.id).unwrap().user_data.unwrap().is_favorite);
    assert!(
        c.favourites("Movie").unwrap().iter().all(|i| i.id != film.id),
        "favourite must leave the list after un-favouriting"
    );
}
