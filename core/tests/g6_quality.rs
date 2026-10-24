//! G6 rules (P-PLAY-12, 13, 20): the transcoding request, the plan, the play request, the menus and the stats rows.
use jf_core::api::*;
use jf_core::platform::{NullPlayer, Player, PlayerStats};
use jf_core::playreq::build_plan;
use jf_core::ui::player::{quality_label, stats_rows, PlayOptions, QUALITY_KBPS, SPEED_STEPS};
use serde_json::json;

fn client() -> Client {
    let d = DeviceInfo { name: "t".into(), id: "d".into(), version: "0".into() };
    Client::with_session(
        "http://h:8096",
        d,
        Session { user_id: "u".into(), user_name: "t".into(), token: "tok".into() },
    )
}

fn fixture<T: serde::de::DeserializeOwned>(n: &str) -> T {
    serde_json::from_str(
        &std::fs::read_to_string(format!("{}/tests/fixtures/{n}", env!("CARGO_MANIFEST_DIR"))).unwrap(),
    )
    .unwrap()
}

#[test]
fn no_cap_is_the_plain_request() {
    assert_eq!(transcode_body(0, false), json!({}));
}

#[test]
fn cap_builds_a_profile() {
    let b = transcode_body(4000, false);
    assert_eq!(b["MaxStreamingBitrate"], 4_000_000);
    assert_eq!(b["DeviceProfile"]["MaxStreamingBitrate"], 4_000_000);
    assert_eq!(b["EnableDirectPlay"], true);
    let tp = &b["DeviceProfile"]["TranscodingProfiles"][0];
    assert_eq!(tp["Protocol"], "hls");
    assert_eq!(tp["Container"], "ts");
    assert_eq!(tp["VideoCodec"], "h264");
    assert_eq!(tp["AudioCodec"], "aac");
    let dp = &b["DeviceProfile"]["DirectPlayProfiles"][0];
    assert_eq!(dp["Container"], "mkv,mp4");
    assert_eq!(dp["VideoCodec"], "h264,hevc");
    assert_eq!(dp["AudioCodec"], "aac,ac3,opus");
}

#[test]
fn forced_transcode_turns_direct_play_off() {
    let b = transcode_body(0, true);
    assert_eq!(b["EnableDirectPlay"], false);
    assert_eq!(b["EnableDirectStream"], false);
    assert_eq!(b["MaxStreamingBitrate"], 20_000_000);
}

#[test]
fn plan_makes_the_transcoding_url_absolute() {
    let raw = json!({"PlaySessionId": "ps", "MediaSources": [
        {"Id": "m", "Bitrate": 9_000_000, "TranscodingUrl": "/videos/i/master.m3u8?MediaSourceId=m"}]});
    let p = plan_from_json(&raw, "http://h:8096", "tok").unwrap();
    assert_eq!(p.transcoding_url.as_deref(), Some("http://h:8096/videos/i/master.m3u8?MediaSourceId=m&api_key=tok"));
    assert_eq!(p.source_bitrate, Some(9_000_000));
    assert_eq!(p.info.play_session_id.as_deref(), Some("ps"));
    let keyed = json!({"MediaSources": [{"Id": "m", "TranscodingUrl": "/v/master.m3u8?ApiKey=x"}]});
    assert_eq!(
        plan_from_json(&keyed, "http://h:8096", "tok").unwrap().transcoding_url.as_deref(),
        Some("http://h:8096/v/master.m3u8?ApiKey=x")
    );
}

#[test]
fn plan_without_transcoding_url_is_direct() {
    let raw = json!({"MediaSources": [{"Id": "m", "SupportsDirectPlay": true}]});
    let p = plan_from_json(&raw, "http://h:8096", "tok").unwrap();
    assert_eq!(p.transcoding_url, None);
}

#[test]
fn request_uses_the_transcoding_url_and_one_audio() {
    let item: Item = fixture("episode_item.json");
    let info: PlaybackInfo = fixture("playbackinfo.json");
    let plan =
        PlaybackPlan { info, transcoding_url: Some("http://h:8096/videos/x/master.m3u8".into()), source_bitrate: None };
    let r = build_plan(&client(), &item, &plan, &PlayOptions::default(), &Default::default()).unwrap();
    // C3-SUBS: the url also names the chosen audio + subtitle (tests/c3_subs.rs).
    assert!(r.url.starts_with("http://h:8096/videos/x/master.m3u8?"), "{}", r.url);
    assert_eq!(r.audio_indexes.len(), 1, "the transcode carries one audio track");
    assert_eq!(r.audio_indexes.first().copied(), r.audio);
    assert!(r.subtitle_indexes.is_empty(), "embedded subs are not in the HLS stream");
    assert_eq!(r.external_subs.len(), 1);
}

#[test]
fn direct_plan_is_the_old_request() {
    let item: Item = fixture("episode_item.json");
    let info: PlaybackInfo = fixture("playbackinfo.json");
    let plan = PlaybackPlan { info: info.clone(), ..Default::default() };
    let a = build_plan(&client(), &item, &plan, &PlayOptions::default(), &Default::default()).unwrap();
    let b = jf_core::playreq::build(&client(), &item, &info, &PlayOptions::default()).unwrap();
    assert_eq!(a, b);
}

#[test]
fn quality_menu() {
    assert_eq!(QUALITY_KBPS, [0, 20000, 10000, 8000, 4000, 2000]);
    assert_eq!(quality_label(Some(0), 0), "Auto");
    assert_eq!(quality_label(None, 0), "Auto");
    assert_eq!(quality_label(None, 3000), "3 Mbit/s");
    assert_eq!(quality_label(Some(8000), 0), "8 Mbit/s");
    assert_eq!(quality_label(Some(0), 3000), "Auto");
}

#[test]
fn speed_steps() {
    assert_eq!(SPEED_STEPS, [0.5, 0.75, 1.0, 1.25, 1.5, 2.0]);
}

#[test]
fn player_defaults() {
    let mut p = NullPlayer::default();
    p.set_speed(2.0); // default no-op
    assert_eq!(p.stats(), PlayerStats::default());
}

#[test]
fn stats_rows_show_what_is_known() {
    let st = PlayerStats {
        width: Some(1920),
        height: Some(1080),
        video_bitrate_bps: Some(4_500_000.0),
        dropped_frames: Some(3),
        buffer_secs: Some(12.4),
    };
    let rows = stats_rows("Transcode", Some("mkv"), Some("h264"), Some(9_000_000), &st);
    let get = |k: &str| rows.iter().find(|(l, _)| *l == k).map(|(_, v)| v.clone());
    assert_eq!(get("Play method").as_deref(), Some("Transcode"));
    assert_eq!(get("Container").as_deref(), Some("mkv"));
    assert_eq!(get("Video").as_deref(), Some("h264 1920×1080"));
    assert_eq!(get("Source bitrate").as_deref(), Some("9.0 Mbit/s"));
    assert_eq!(get("Bitrate").as_deref(), Some("4.5 Mbit/s"));
    assert_eq!(get("Dropped frames").as_deref(), Some("3"));
    assert_eq!(get("Buffer").as_deref(), Some("12 s"));
    let bare = stats_rows("DirectPlay", None, None, None, &PlayerStats::default());
    assert!(bare.iter().all(|(l, _)| *l != "Dropped frames" && *l != "Buffer"), "unknown rows are left out");
}
