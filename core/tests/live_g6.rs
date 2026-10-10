//! G6 live test (P-PLAY-13). Against the throwaway server only; skipped without `JF_TEST_URL`.
//! A cap below an Archive film's bitrate returns a TranscodingUrl, and the URL fetches an HLS playlist. (The Archive
//! films are ~0.3 Mbit/s, so a literal 1 Mbit cap would correctly direct play; the cap is half the source bitrate.)
use jf_core::api::*;

#[test]
fn capped_playback_info_returns_a_playable_hls_url() {
    let Ok(u) = std::env::var("JF_TEST_URL") else {
        eprintln!("JF_TEST_URL unset: live test skipped");
        return;
    };
    let dev = DeviceInfo { name: "live-g6".into(), id: "live-g6-device".into(), version: "0.1.0".into() };
    let (mut c, _) = Client::connect(&u, dev).unwrap();
    c.sign_in("viewer", "view-pass-1").unwrap();
    let arch = c.views().unwrap().into_iter().find(|v| v.name == "Archive").unwrap();
    let film = c.library_page(&arch.id, 0, 1).unwrap().items.remove(0);

    let direct = c.playback_plan(&film.id, 0, false).unwrap();
    assert_eq!(direct.transcoding_url, None, "no cap = direct play");

    let cap_kbps = (direct.source_bitrate.expect("source bitrate") / 2000).max(1) as u32;
    let above = c.playback_plan(&film.id, cap_kbps * 4, false).unwrap();
    assert_eq!(above.transcoding_url, None, "a cap above the source bitrate direct plays");
    let plan = c.playback_plan(&film.id, cap_kbps, false).unwrap();
    let url = plan.transcoding_url.expect("a cap below the source bitrate must return a TranscodingUrl");
    assert!(url.starts_with(&u), "{url}");
    assert!(url.contains("m3u8"), "{url}");

    let body = ureq::get(&url).call().unwrap().body_mut().read_to_string().unwrap();
    assert!(body.starts_with("#EXTM3U"), "not an HLS playlist: {body}");

    let forced = c.playback_plan(&film.id, 0, true).unwrap();
    assert!(forced.transcoding_url.is_some(), "forced transcode must return a TranscodingUrl");
}
