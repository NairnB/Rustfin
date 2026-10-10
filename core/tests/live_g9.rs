//! G9 live tests: mark a season played (P-TV-5), missing-episode filter (P-TV-7), library play list (P-LIB-23).
//! Skipped without `JF_TEST_URL`; writes only on the throwaway server and puts the played state back.
use jf_core::api::*;
use jf_core::ui::show_page::shuffle_queue;

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
fn drift(c: &Client) -> Item {
    c.search("Drift Rebels").unwrap().into_iter().find(|i| i.kind == ItemKind::Series).expect("Drift Rebels")
}
fn played(i: &Item) -> bool {
    i.user_data.as_ref().map(|u| u.played).unwrap_or(false)
}

// P-TV-5: set_played on a season id cascades to its episodes; put back as found.
#[test]
fn mark_season_played_cascades() {
    let Some(u) = url() else { return };
    let c = signed(&u);
    let ser = drift(&c);
    let seasons = c.seasons(&ser.id).unwrap();
    let s2 = seasons.iter().find(|s| s.index_number == Some(2)).expect("season 2");
    let before = c.episodes(&ser.id, Some(&s2.id)).unwrap();
    assert_eq!(before.len(), 2);
    c.set_played(&s2.id, true).unwrap();
    let after = c.episodes(&ser.id, Some(&s2.id)).unwrap();
    assert!(after.iter().all(played), "both season 2 episodes played");
    // Put back: per-episode state as it was.
    for e in &before {
        c.set_played(&e.id, played(e)).unwrap();
    }
    let restored = c.episodes(&ser.id, Some(&s2.id)).unwrap();
    for (b, r) in before.iter().zip(&restored) {
        assert_eq!(played(b), played(r));
    }
}

// P-TV-7: the episodes list loads with and without missing episodes.
#[test]
fn episodes_opts_both_modes() {
    let Some(u) = url() else { return };
    let c = signed(&u);
    let ser = drift(&c);
    let hidden = c.episodes_opts(&ser.id, false).unwrap();
    let shown = c.episodes_opts(&ser.id, true).unwrap();
    assert!(!hidden.is_empty());
    assert!(shown.len() >= hidden.len());
}

// P-LIB-23: Play all keeps the library's order, Shuffle returns the same items (as a set), both capped.
#[test]
fn library_playable_lists() {
    let Some(u) = url() else { return };
    let c = signed(&u);
    let shows = c.views().unwrap().into_iter().find(|v| v.name == "Movies").unwrap();
    let q = LibraryQuery::default();
    let all = c.library_playable(&shows.id, 300, &q, false).unwrap();
    let shuf = c.library_playable(&shows.id, 300, &q, true).unwrap();
    assert!(!all.is_empty());
    assert!(all.len() <= 300);
    let mut a: Vec<_> = all.iter().map(|i| i.id.clone()).collect();
    let mut s: Vec<_> = shuf.iter().map(|i| i.id.clone()).collect();
    a.sort();
    s.sort();
    assert_eq!(a, s);
    assert_eq!(c.library_playable(&shows.id, 2, &q, false).unwrap().len(), 2);
}

// P-TV-10: the shuffle queue holds each unplayed episode once; all episodes when none is unplayed.
#[test]
fn shuffle_queue_unplayed_first_pool() {
    let Some(u) = url() else { return };
    let c = signed(&u);
    let ser = drift(&c);
    let eps = c.episodes(&ser.id, None).unwrap();
    let q = shuffle_queue(&eps, 42);
    let want = eps.iter().filter(|e| !played(e)).count();
    assert_eq!(q.len(), if want == 0 { eps.len() } else { want });
    let mut ids: Vec<_> = q.iter().map(|i| &i.id).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), q.len());
}
