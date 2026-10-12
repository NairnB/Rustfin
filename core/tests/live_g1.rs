//! G1 live tests (P-LIB-2..9): sort + filter queries against the throwaway server ONLY (`JF_TEST_URL`, user
//! tester / test-pass-1). Skipped without JF_TEST_URL. Run with --test-threads=1. Server writes are put back.
use jf_core::api::*;

fn url() -> Option<String> {
    let u = std::env::var("JF_TEST_URL").ok();
    if u.is_none() {
        eprintln!("JF_TEST_URL unset: live test skipped");
    }
    u
}
fn dev() -> DeviceInfo {
    DeviceInfo { name: "live-g1".into(), id: "live-g1-device".into(), version: "0.1.0".into() }
}
fn signed(base: &str) -> Client {
    let (mut c, _) = Client::connect(base, dev()).unwrap();
    c.sign_in("tester", "test-pass-1").unwrap();
    c
}
fn view(c: &Client, name: &str) -> Item {
    c.views().unwrap().into_iter().find(|v| v.name == name).unwrap()
}

#[test]
fn archive_sort_and_random() {
    let Some(u) = url() else { return };
    let c = signed(&u);
    let arch = view(&c, "Archive");

    let name_asc = c.library_query(&arch.id, 0, 100, &LibraryQuery::default()).unwrap();
    assert_eq!(name_asc.total_record_count, 500);
    assert_eq!(name_asc.items[0].name, "Archive Film 001", "default sort is Name ascending");

    let date_desc = c
        .library_query(
            &arch.id,
            0,
            100,
            &LibraryQuery { sort: SortBy::DateAdded, descending: true, ..Default::default() },
        )
        .unwrap();
    assert_eq!(date_desc.total_record_count, 500);
    assert_ne!(name_asc.items[0].name, date_desc.items[0].name, "DateCreated descending must reorder the Archive");

    let random =
        c.library_query(&arch.id, 0, 100, &LibraryQuery { sort: SortBy::Random, ..Default::default() }).unwrap();
    assert_eq!(random.total_record_count, 500, "Random returns the same count");
    assert_eq!(random.items.len(), 100);
}

/// Puts a film's watched flag back as it was, also when an assertion fails (a failed run used to leave the film
/// watched for tester, and later runs failed on the counts).
struct PutBack {
    client: Client,
    id: String,
    was_played: bool,
}

impl Drop for PutBack {
    fn drop(&mut self) {
        let _ = self.client.set_played(&self.id, self.was_played);
    }
}

#[test]
fn mixed_unplayed_filter_and_put_back() {
    let Some(u) = url() else { return };
    let c = signed(&u);
    let mixed = view(&c, "Mixed");

    let all = c.library_query(&mixed.id, 0, 100, &LibraryQuery::default()).unwrap();
    assert_eq!(all.total_record_count, 9);
    // An unwatched film, so the test never depends on (or changes) a mark it did not make.
    let film = all
        .items
        .iter()
        .find(|i| i.kind == ItemKind::Movie && !i.user_data.as_ref().is_some_and(|d| d.played))
        .expect("an unwatched film")
        .clone();
    let query =
        |f: Filters| c.library_query(&mixed.id, 0, 100, &LibraryQuery { filters: f, ..Default::default() }).unwrap();
    let unplayed_before = query(Filters { unplayed: true, ..Default::default() }).total_record_count;

    let _back = PutBack { client: c.clone(), id: film.id.clone(), was_played: false };
    c.set_played(&film.id, true).unwrap();

    let unplayed = query(Filters { unplayed: true, ..Default::default() });
    assert_eq!(unplayed.total_record_count, unplayed_before - 1, "unplayed filter excludes the marked film");
    assert!(unplayed.items.iter().all(|i| i.id != film.id));

    let played = query(Filters { played: true, ..Default::default() });
    assert!(played.items.iter().any(|i| i.id == film.id), "played filter has the marked film");
    assert!(played.items.iter().all(|i| unplayed.items.iter().all(|x| x.id != i.id)), "the two filters never overlap");
    assert_eq!(played.total_record_count + unplayed.total_record_count, 9, "played + unplayed = everything");

    c.set_played(&film.id, false).unwrap();
    let back = c.library_query(&mixed.id, 0, 100, &LibraryQuery::default()).unwrap();
    assert_eq!(back.total_record_count, 9, "put back after the test");
}
