//! F3 live test (R-F3-1/3, R-TAG-6): the tester (admin) sets 0 BBY on "Star Drift - Beacon", the viewer's fetch shows
//! "1977 · 0 BBY" style year lines, the tester clears it. Throwaway server ONLY (`JF_TEST_URL`); skipped without it.
//! The story tag is removed at the end, even when an assert fails.
use jf_core::api::*;
use jf_core::story::year_line;
use jf_core::tags::{with_story, Era, RfTags, StoryYear};

fn url() -> Option<String> {
    let u = std::env::var("JF_TEST_URL").ok();
    if u.is_none() {
        eprintln!("JF_TEST_URL unset: live test skipped");
    }
    u
}

fn signed(base: &str, user: &str, pass: &str) -> Client {
    let (mut c, _) = Client::connect(
        base,
        DeviceInfo { name: "live-f3".into(), id: "live-f3-device".into(), version: "0.1.0".into() },
    )
    .unwrap();
    c.sign_in(user, pass).unwrap();
    c
}

/// Removes the story tag when dropped (panic or not).
struct Cleanup {
    base: String,
    id: String,
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        let c = signed(&self.base, "tester", "test-pass-1");
        let _ = c.update_tags(&self.id, |t| with_story(t, None));
    }
}

#[test]
fn story_year_set_seen_by_viewer_and_cleared() {
    let Some(u) = url() else { return };
    let admin = signed(&u, "tester", "test-pass-1");
    let beacon = admin
        .search("Beacon")
        .unwrap()
        .into_iter()
        .find(|i| i.name == "Star Drift - Beacon")
        .expect("Star Drift - Beacon on the test server");
    // leftovers from an earlier failed run
    admin.update_tags(&beacon.id, |t| with_story(t, None)).unwrap();
    let _guard = Cleanup { base: u.clone(), id: beacon.id.clone() };

    admin.update_tags(&beacon.id, |t| with_story(t, Some(StoryYear { n: 0, era: Era::Bby }))).unwrap();

    let viewer = signed(&u, "viewer", "view-pass-1");
    let seen = viewer.item(&beacon.id).unwrap();
    assert_eq!(RfTags::parse(&seen.tags).story, Some(StoryYear { n: 0, era: Era::Bby }));
    let line = year_line(&seen, true, true);
    assert!(line.ends_with("0 BBY"), "year line was {line:?}");
    if let Some(y) = seen.production_year {
        assert_eq!(line, format!("{y} · 0 BBY"));
    }
    assert_eq!(year_line(&seen, false, true), "0 BBY");

    // a non-admin cannot write it
    assert_eq!(viewer.update_tags(&beacon.id, |t| with_story(t, None)), Err(ApiError::Forbidden));

    admin.update_tags(&beacon.id, |t| with_story(t, None)).unwrap();
    let cleared = viewer.item(&beacon.id).unwrap();
    assert_eq!(RfTags::parse(&cleared.tags).story, None);
    assert_eq!(year_line(&cleared, true, true), seen.production_year.map(|y| y.to_string()).unwrap_or_default());
}
