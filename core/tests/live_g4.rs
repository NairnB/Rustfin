//! G4 live tests: `previous_episode` (P-PLAY-16 / P-TV-9). Skipped without `JF_TEST_URL`; runs against the
//! throwaway server only.
use jf_core::api::*;

fn url() -> Option<String> {
    let u = std::env::var("JF_TEST_URL").ok();
    if u.is_none() {
        eprintln!("JF_TEST_URL unset: live test skipped");
    }
    u
}
fn dev() -> DeviceInfo {
    DeviceInfo { name: "live-test".into(), id: "live-test-device".into(), version: "0.1.0".into() }
}
fn signed(base: &str) -> Client {
    let (mut c, _) = Client::connect(base, dev()).unwrap();
    c.sign_in("tester", "test-pass-1").unwrap();
    c
}
fn series(c: &Client) -> Item {
    let shows = c.views().unwrap().into_iter().find(|v| v.name == "Shows").unwrap();
    c.library_page(&shows.id, 0, 10).unwrap().items.remove(0)
}
fn episode(c: &Client, s: i32, e: i32) -> Item {
    let ser = series(c);
    c.episodes(&ser.id, None)
        .unwrap()
        .into_iter()
        .find(|x| x.parent_index_number == Some(s) && x.index_number == Some(e))
        .unwrap()
}

// P-PLAY-16 / P-TV-9: previous_episode mirrors next_episode, both edges in series order.
#[test]
fn previous_and_next_episode() {
    let Some(u) = url() else { return };
    let c = signed(&u);
    let ser = series(&c);
    assert_eq!(ser.name, "Night Shift Stories");
    // Middle episode has both neighbours.
    let mid = episode(&c, 1, 2);
    let prev = c.previous_episode(&ser.id, &mid.id).unwrap().unwrap();
    assert_eq!((prev.parent_index_number, prev.index_number), (Some(1), Some(1)));
    let next = c.next_episode(&ser.id, &mid.id).unwrap().unwrap();
    assert_eq!((next.parent_index_number, next.index_number), (Some(1), Some(3)));
    // First episode has no previous.
    let first = episode(&c, 1, 1);
    assert!(c.previous_episode(&ser.id, &first.id).unwrap().is_none());
    // Last episode has no next (already covered by live.rs, kept here for the pair).
    let last = episode(&c, 2, 3);
    assert!(c.next_episode(&ser.id, &last.id).unwrap().is_none());
    // Across the season boundary, previous of S2E1 is S1E3.
    let s2e1 = episode(&c, 2, 1);
    let prev = c.previous_episode(&ser.id, &s2e1.id).unwrap().unwrap();
    assert_eq!((prev.parent_index_number, prev.index_number), (Some(1), Some(3)));
}
