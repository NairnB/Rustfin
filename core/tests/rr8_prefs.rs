//! RR8 rules (P-USER-1, P-USER-2, P-USER-4): picture upload helpers, fMP4-HLS profile, next-up request,
//! screensaver settings and the idle rule.
use jf_core::api::*;
use jf_core::settings::Settings;
use jf_core::ui::screensaver::{idle_due, Screensaver};
use serde_json::json;

#[test]
fn base64_matches_the_standard_vectors() {
    assert_eq!(base64_encode(b""), "");
    assert_eq!(base64_encode(b"f"), "Zg==");
    assert_eq!(base64_encode(b"fo"), "Zm8=");
    assert_eq!(base64_encode(b"foo"), "Zm9v");
    assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
}

#[test]
fn picture_type_comes_from_the_bytes() {
    assert_eq!(image_content_type(b"\x89PNG\r\n\x1a\n...."), Some("image/png"));
    assert_eq!(image_content_type(&[0xff, 0xd8, 0xff, 0xe0, 0]), Some("image/jpeg"));
    assert_eq!(image_content_type(b"GIF89a...."), Some("image/gif"));
    assert_eq!(image_content_type(b"RIFF\0\0\0\0WEBPVP8 "), Some("image/webp"));
    assert_eq!(image_content_type(b"not a picture"), None);
}

#[test]
fn fmp4_changes_only_the_transcoding_container() {
    assert_eq!(transcode_body_with(0, false, true), json!({}), "no cap, no force: still the plain request");
    let ts = transcode_body_with(4000, false, false);
    assert_eq!(ts, transcode_body(4000, false));
    let tp = &ts["DeviceProfile"]["TranscodingProfiles"][0];
    assert_eq!(tp["Container"], "ts");
    let f = transcode_body_with(4000, false, true);
    let fp = &f["DeviceProfile"]["TranscodingProfiles"][0];
    assert_eq!(fp["Container"], "mp4");
    assert_eq!(fp["Protocol"], "hls");
    assert_eq!(f["MaxStreamingBitrate"], ts["MaxStreamingBitrate"]);
}

#[test]
fn next_up_request_carries_rewatching_and_cutoff() {
    let plain = next_up_path("u1", 20, false, 0, 0);
    assert!(plain.starts_with("/Shows/NextUp?userId=u1&Limit=20"), "{plain}");
    assert!(!plain.contains("nextUpDateCutoff") && !plain.contains("enableRewatching"), "{plain}");
    // 2026-10-11 12:00:00 UTC
    let now = 1_791_720_000;
    let p = next_up_path("u1", 20, true, 30, now);
    assert!(p.contains("enableRewatching=true"), "{p}");
    assert!(p.contains("nextUpDateCutoff=2026-09-11"), "{p}");
}

#[test]
fn days_ago_crosses_months_and_years() {
    assert_eq!(date_days_ago(1_791_720_000, 0), "2026-10-11");
    assert_eq!(date_days_ago(1_791_720_000, 283), "2026-01-01");
    assert_eq!(date_days_ago(1_791_720_000, 284), "2025-12-31");
}

#[test]
fn new_settings_have_the_old_behaviour_by_default() {
    let s = Settings::default();
    assert!(!s.prefer_fmp4 && !s.next_up_rewatching);
    assert_eq!(s.next_up_days, 0);
    assert_eq!(s.screensaver, Screensaver::None);
    assert_eq!(s.screensaver_minutes, 3);
    let old: Settings = serde_json::from_str(r#"{"device_id":"x"}"#).unwrap();
    assert_eq!(old.screensaver, Screensaver::None);
    let s = Settings {
        prefer_fmp4: true,
        next_up_rewatching: true,
        next_up_days: 90,
        screensaver: Screensaver::Backdrops,
        screensaver_minutes: 10,
        ..Default::default()
    };
    let back: Settings = serde_json::from_slice(&serde_json::to_vec(&s).unwrap()).unwrap();
    assert_eq!(back, s);
}

#[test]
fn idle_rule() {
    assert!(!idle_due(Screensaver::None, 3, 0.0, 10_000.0, false), "off never starts");
    assert!(!idle_due(Screensaver::Backdrops, 3, 0.0, 179.0, false), "not yet");
    assert!(idle_due(Screensaver::Backdrops, 3, 0.0, 180.0, false));
    assert!(idle_due(Screensaver::Logo, 1, 100.0, 160.0, false));
    assert!(!idle_due(Screensaver::Backdrops, 3, 0.0, 10_000.0, true), "never over the player");
}
