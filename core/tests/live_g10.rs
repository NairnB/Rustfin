//! G10 live tests: `media_segments` (P-PLAY-11). Skipped without `JF_TEST_URL`; reads only.
use jf_core::api::*;

#[test]
fn media_segments_decodes_and_is_empty_on_jf_test() {
    let Ok(base) = std::env::var("JF_TEST_URL") else {
        eprintln!("JF_TEST_URL unset: live test skipped");
        return;
    };
    let dev = DeviceInfo { name: "live-test".into(), id: "live-test-device".into(), version: "0.1.0".into() };
    let (mut c, _) = Client::connect(&base, dev).unwrap();
    c.sign_in("tester", "test-pass-1").unwrap();
    let shows = c.views().unwrap().into_iter().find(|v| v.name == "Shows").unwrap();
    let ser = c.library_page(&shows.id, 0, 10).unwrap().items.remove(0);
    let ep = c.episodes(&ser.id, None).unwrap().remove(0);
    // jf-test has no segment data: the call must decode and give nothing.
    assert!(c.media_segments(&ep.id).unwrap().is_empty());
}
