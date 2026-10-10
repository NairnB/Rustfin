//! F4 live tests (R-F4-1/2, R-TAG-6): "My order" writes against the throwaway server ONLY (`JF_TEST_URL`,
//! users tester / test-pass-1 admin, viewer / view-pass-1 non-admin). Skipped without JF_TEST_URL. Server
//! writes are put back (every `rf-order` tag the test wrote is removed at the end).
use jf_core::api::*;
use jf_core::order::{move_writes, sort_my_order};
use jf_core::tags::{place_id, with_order, RfTags};

fn url() -> Option<String> {
    let u = std::env::var("JF_TEST_URL").ok();
    if u.is_none() {
        eprintln!("JF_TEST_URL unset: live test skipped");
    }
    u
}

fn dev() -> DeviceInfo {
    DeviceInfo { name: "live-f4".into(), id: "live-f4-device".into(), version: "0.1.0".into() }
}

fn signed(base: &str, user: &str, pass: &str) -> Client {
    let (mut c, _) = Client::connect(base, dev()).unwrap();
    c.sign_in(user, pass).unwrap();
    c
}

fn view(c: &Client, name: &str) -> Item {
    c.views().unwrap().into_iter().find(|v| v.name == name).unwrap()
}

/// Remove every `rf-order*` tag (any place), leaving other tags untouched.
fn strip_order(tags: &[String]) -> Vec<String> {
    tags.iter().filter(|t| !t.starts_with("rf-order")).cloned().collect()
}

#[test]
fn move_third_film_to_front_is_one_write_and_viewer_is_forbidden() {
    let Some(u) = url() else { return };
    let c = signed(&u, "tester", "test-pass-1");
    let mixed = view(&c, "Mixed");
    let place = place_id(&mixed.id);

    let films: Vec<Item> = c
        .library_query(&mixed.id, 0, 100, &LibraryQuery::default())
        .unwrap()
        .items
        .into_iter()
        .filter(|i| i.kind == ItemKind::Movie)
        .take(3)
        .collect();
    assert_eq!(films.len(), 3, "Mixed needs at least three films");

    // Number the three films 10 / 20 / 30 (R-F4-1).
    for (i, f) in films.iter().enumerate() {
        let n = (i as u32 + 1) * 10;
        c.update_tags(&f.id, |t| with_order(t, &place, Some(n))).unwrap();
    }

    // Re-read so the shown items carry the tags just written.
    let films: Vec<Item> = films.iter().map(|f| c.item(&f.id).unwrap()).collect();
    let mut shown = films.clone();
    sort_my_order(&mut shown, &place);
    // Moving the third film to the front lands between "none" and 10 → exactly one write of 5.
    assert_eq!(move_writes(&shown, &place, 2, 0), vec![(films[2].id.clone(), 5)]);

    // Apply that one write and confirm the tag lands on the server.
    c.update_tags(&films[2].id, |t| with_order(t, &place, Some(5))).unwrap();
    let after = c.item(&films[2].id).unwrap();
    assert_eq!(RfTags::parse(&after.tags).order.get(&place), Some(&5));

    // Clean up every rf-order tag the test wrote.
    for f in &films {
        c.update_tags(&f.id, strip_order).unwrap();
    }

    // A non-admin cannot write tags (R-TAG-6).
    let v = signed(&u, "viewer", "view-pass-1");
    assert_eq!(v.update_tags(&films[0].id, |t| t.to_vec()), Err(ApiError::Forbidden));
}
