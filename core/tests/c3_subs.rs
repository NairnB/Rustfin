//! C3-SUBS rules (R-SUBS-1..4): the transcoding path names the chosen audio + subtitle in the request and in the
//! stream URL, so the server never burns in its own default subtitle.
use jf_core::api::*;
use jf_core::playreq::{build_plan, build_plan_burnt, request_indexes};
use jf_core::ui::player::{subtitle_change_replans, track_change_force, track_change_replans, PlayOptions};

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

// playbackinfo.json: 0 = external ger sub, 2 = eng audio, 3 = fre audio, 4 = embedded eng sub; default sub 0.
fn plan(url: &str) -> PlaybackPlan {
    PlaybackPlan { info: fixture("playbackinfo.json"), transcoding_url: Some(url.into()), source_bitrate: None }
}

fn opts(audio: Option<i32>, subtitle: Option<Option<i32>>) -> PlayOptions {
    PlayOptions { audio, subtitle, ..Default::default() }
}

fn count(url: &str, key: &str) -> usize {
    url.split_once('?').map(|q| q.1).unwrap_or("").split('&').filter(|p| p.starts_with(&format!("{key}="))).count()
}

// ---- R-SUBS-1: the request ----

#[test]
fn request_path_names_the_indexes() {
    assert_eq!(playback_info_path("u", "i", None, None), "/Items/i/PlaybackInfo?userId=u");
    assert_eq!(
        playback_info_path("u", "i", Some(3), Some(4)),
        "/Items/i/PlaybackInfo?userId=u&AudioStreamIndex=3&SubtitleStreamIndex=4"
    );
    assert_eq!(playback_info_path("u", "i", None, Some(-1)), "/Items/i/PlaybackInfo?userId=u&SubtitleStreamIndex=-1");
}

#[test]
fn request_indexes_embedded_off_and_external() {
    let info: PlaybackInfo = fixture("playbackinfo.json");
    let i = Some(&info);
    // an embedded subtitle the user chose → its index
    assert_eq!(request_indexes(i, None, Some(3), Some(Some(4))), (Some(3), Some(4)));
    // off → -1
    assert_eq!(request_indexes(i, None, Some(2), Some(None)), (Some(2), Some(-1)));
    // an external subtitle the client loads itself → -1
    assert_eq!(request_indexes(i, None, None, Some(Some(0))), (None, Some(-1)));
    // no choice made → nothing sent (server default)
    assert_eq!(request_indexes(i, None, None, None), (None, None));
    // source not known yet (first request): the chosen index goes as given; off is still -1
    assert_eq!(request_indexes(None, None, None, Some(Some(4))), (None, Some(4)));
    assert_eq!(request_indexes(None, None, None, Some(None)), (None, Some(-1)));
}

// ---- R-SUBS-2: the URL ----

#[test]
fn url_gets_the_subtitle_and_audio() {
    let p = plan("http://h:8096/videos/x/master.m3u8?MediaSourceId=m&api_key=tok");
    let r = build_plan(&client(), &Item::default(), &p, &opts(Some(3), Some(Some(4))), &Default::default()).unwrap();
    assert_eq!(count(&r.url, "SubtitleStreamIndex"), 1);
    assert_eq!(count(&r.url, "AudioStreamIndex"), 1);
    assert!(r.url.contains("SubtitleStreamIndex=4") && r.url.contains("AudioStreamIndex=3"), "{}", r.url);
    assert!(r.url.contains("MediaSourceId=m") && r.url.contains("api_key=tok"));
}

#[test]
fn url_off_and_external_are_minus_one() {
    let p = plan("http://h:8096/v/master.m3u8?api_key=tok");
    let off = build_plan(&client(), &Item::default(), &p, &opts(None, Some(None)), &Default::default()).unwrap();
    assert!(off.url.contains("SubtitleStreamIndex=-1"), "{}", off.url);
    let ext = build_plan(&client(), &Item::default(), &p, &opts(None, Some(Some(0))), &Default::default()).unwrap();
    assert!(ext.url.contains("SubtitleStreamIndex=-1"), "{}", ext.url);
    assert_eq!(ext.subtitle, Some(0), "the external subtitle still loads in the client");
}

#[test]
fn url_overrides_what_the_server_burnt_in() {
    // The server's own url names its default (Japanese) subtitle and audio; ours wins, no duplicates.
    let p =
        plan("http://h:8096/v/master.m3u8?SubtitleStreamIndex=0&AudioStreamIndex=2&api_key=tok&SubtitleMethod=Encode");
    let r = build_plan(&client(), &Item::default(), &p, &opts(Some(3), Some(None)), &Default::default()).unwrap();
    assert_eq!(count(&r.url, "SubtitleStreamIndex"), 1);
    assert_eq!(count(&r.url, "AudioStreamIndex"), 1);
    assert!(r.url.contains("SubtitleStreamIndex=-1") && r.url.contains("AudioStreamIndex=3"), "{}", r.url);
    assert!(r.url.contains("SubtitleMethod=Encode"));
}

#[test]
fn url_uses_the_preference_when_the_page_chose_nothing() {
    // opts.subtitle None → the user's prefs / the source default (external 0 here) → -1; audio = source default.
    let p = plan("http://h:8096/v/master.m3u8?api_key=tok");
    let r = build_plan(&client(), &Item::default(), &p, &PlayOptions::default(), &Default::default()).unwrap();
    assert!(r.url.contains("SubtitleStreamIndex=-1"), "{}", r.url);
    assert_eq!(count(&r.url, "AudioStreamIndex"), 1);
}

#[test]
fn stream_indexes_helper_replaces_in_place() {
    assert_eq!(with_stream_indexes("http://h/m.m3u8", None, None, false), "http://h/m.m3u8");
    assert_eq!(
        with_stream_indexes("http://h/m.m3u8", Some(1), Some(-1), false),
        "http://h/m.m3u8?AudioStreamIndex=1&SubtitleStreamIndex=-1"
    );
    assert_eq!(
        with_stream_indexes("http://h/m.m3u8?a=b&SubtitleStreamIndex=5", None, Some(2), false),
        "http://h/m.m3u8?a=b&SubtitleStreamIndex=2"
    );
}

#[test]
fn embedded_choice_asks_for_encode() {
    let p = plan("http://h:8096/v/master.m3u8?SubtitleMethod=External&api_key=tok");
    let (r, burnt) =
        build_plan_burnt(&client(), &Item::default(), &p, &opts(Some(3), Some(Some(4))), &Default::default()).unwrap();
    assert_eq!(burnt, Some(4));
    assert_eq!(count(&r.url, "SubtitleMethod"), 1);
    assert!(r.url.contains("SubtitleMethod=Encode"), "{}", r.url);
    assert_eq!(r.subtitle, None, "nothing for mpv to load, it is burnt in");
    // off / external: no Encode in the url
    let off = build_plan(&client(), &Item::default(), &p, &opts(None, Some(None)), &Default::default()).unwrap();
    assert!(!off.url.contains("SubtitleMethod=Encode"), "{}", off.url);
}

#[test]
fn external_choice_stays_client_side() {
    let p = plan("http://h:8096/v/master.m3u8?api_key=tok");
    let (r, burnt) =
        build_plan_burnt(&client(), &Item::default(), &p, &opts(None, Some(Some(0))), &Default::default()).unwrap();
    assert_eq!(burnt, None);
    assert_eq!(r.subtitle, Some(0));
    assert!(r.url.contains("SubtitleStreamIndex=-1") && !r.url.contains("Encode"), "{}", r.url);
}

#[test]
fn a_stale_embedded_index_is_not_burnt() {
    let p = plan("http://h:8096/v/master.m3u8?api_key=tok");
    let (r, burnt) =
        build_plan_burnt(&client(), &Item::default(), &p, &opts(None, Some(Some(99))), &Default::default()).unwrap();
    assert_eq!(burnt, None);
    assert!(r.url.contains("SubtitleStreamIndex=-1"), "{}", r.url);
}

// ---- R-SUBS-3: a track change while transcoding re-plans (same path as a quality change: reload) ----

#[test]
fn track_change_replans_only_while_transcoding() {
    assert!(track_change_replans(true));
    assert!(!track_change_replans(false));
}

#[test]
fn track_change_force_keeps_a_fallback_transcode_transcoding() {
    // the P-PLAY-13 fallback transcodes only because the request forced it; a track change must force again
    assert!(track_change_force(true, true));
    assert!(!track_change_force(true, false), "a transcode the server chose needs no force");
    assert!(!track_change_force(false, false));
}

#[test]
fn subtitle_change_replans_only_when_the_burnt_stream_changes() {
    assert!(subtitle_change_replans(true, Some(-1), Some(4)), "off → embedded burns it in");
    assert!(subtitle_change_replans(true, Some(4), Some(-1)), "embedded → off drops the burn");
    assert!(!subtitle_change_replans(true, Some(-1), Some(-1)), "off → external stays client side");
    assert!(!subtitle_change_replans(false, Some(-1), Some(4)), "direct play switches in place");
}

#[test]
fn burnt_subtitle_survives_the_next_replan() {
    // start on a transcode with the embedded English sub: the player keeps index 4, not Off
    let p = plan("http://h:8096/v/master.m3u8?api_key=tok");
    let (_, burnt) =
        build_plan_burnt(&client(), &Item::default(), &p, &opts(Some(2), Some(Some(4))), &Default::default()).unwrap();
    let kept = Some(burnt.or(None));
    assert_eq!(kept, Some(Some(4)));
    // an audio change / quality change re-plans with that choice: the request and the new url still name 4
    assert_eq!(request_indexes(Some(&p.info), None, Some(3), kept), (Some(3), Some(4)));
    let (r2, burnt2) =
        build_plan_burnt(&client(), &Item::default(), &p, &opts(Some(3), kept), &Default::default()).unwrap();
    assert_eq!(burnt2, Some(4));
    assert!(r2.url.contains("SubtitleStreamIndex=4") && r2.url.contains("AudioStreamIndex=3"), "{}", r2.url);
}

// ---- R-SUBS-4: direct play is unchanged ----

#[test]
fn direct_plan_url_is_untouched() {
    let info: PlaybackInfo = fixture("playbackinfo.json");
    let p = PlaybackPlan { info: info.clone(), ..Default::default() };
    let a = build_plan(&client(), &Item::default(), &p, &opts(Some(3), Some(Some(4))), &Default::default()).unwrap();
    let b = jf_core::playreq::build(&client(), &Item::default(), &info, &opts(Some(3), Some(Some(4)))).unwrap();
    assert_eq!(a, b);
    assert!(!a.url.contains("SubtitleStreamIndex"));
}

#[test]
fn transcode_request_carries_subtitle_index() {
    assert_eq!(request_indexes(None, None, Some(2), Some(Some(4))), (Some(2), Some(4)));
    assert!(playback_info_path("u", "i", Some(2), Some(4)).contains("SubtitleStreamIndex=4"));
}

#[test]
fn subtitles_off_sends_minus_one() {
    assert_eq!(request_indexes(None, None, None, Some(None)).1, Some(-1));
    assert!(playback_info_path("u", "i", None, Some(-1)).ends_with("SubtitleStreamIndex=-1"));
}

#[test]
fn transcoding_url_gets_subtitle_index_once() {
    let p = plan("http://h:8096/v/master.m3u8?SubtitleStreamIndex=0&SubtitleStreamIndex=1&api_key=tok");
    let r = build_plan(&client(), &Item::default(), &p, &opts(None, Some(Some(4))), &Default::default()).unwrap();
    assert_eq!(count(&r.url, "SubtitleStreamIndex"), 1);
    assert!(r.url.contains("SubtitleStreamIndex=4"), "{}", r.url);
}

#[test]
fn audio_index_in_request_and_url() {
    assert!(playback_info_path("u", "i", Some(3), None).contains("AudioStreamIndex=3"));
    let p = plan("http://h:8096/v/master.m3u8?AudioStreamIndex=2&api_key=tok");
    let r = build_plan(&client(), &Item::default(), &p, &opts(Some(3), None), &Default::default()).unwrap();
    assert_eq!(count(&r.url, "AudioStreamIndex"), 1);
    assert!(r.url.contains("AudioStreamIndex=3"), "{}", r.url);
}

#[test]
fn direct_play_request_unchanged() {
    assert_eq!(transcode_body_with(0, false, false), serde_json::json!({}));
    assert_eq!(playback_info_path("u", "i", None, None), "/Items/i/PlaybackInfo?userId=u");
    let body = transcode_body_with(8000, false, false);
    assert_eq!(body["DeviceProfile"]["SubtitleProfiles"][0]["Method"], "External");
}

// ---- R-SUBS-4 / finding 1: the request body never forces a burn-in profile, so direct play stays possible ----

/// Serves one canned PlaybackInfo answer on a local port and returns the request body the client sent.
fn capture_playback_body(sub: Option<i32>, cap: u32) -> serde_json::Value {
    use std::io::{Read, Write};
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let t = std::thread::spawn(move || {
        let (mut s, _) = l.accept().unwrap();
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            let n = s.read(&mut chunk).unwrap();
            buf.extend_from_slice(&chunk[..n]);
            let text = String::from_utf8_lossy(&buf).to_string();
            if let Some((head, body)) = text.split_once("\r\n\r\n") {
                let want: usize = head
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse().unwrap())
                    })
                    .unwrap_or(0);
                if body.len() >= want {
                    let ans = r#"{"MediaSources":[],"PlaySessionId":"p"}"#;
                    write!(s, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{ans}", ans.len()).unwrap();
                    return serde_json::from_str(body).unwrap_or(serde_json::json!({}));
                }
            }
        }
    });
    let d = DeviceInfo { name: "t".into(), id: "d".into(), version: "0".into() };
    let c = Client::with_session(
        &format!("http://127.0.0.1:{port}"),
        d,
        Session { user_id: "u".into(), user_name: "t".into(), token: "tok".into() },
    );
    let _ = c.playback_plan_for("i", cap, false, false, Some(1), sub);
    t.join().unwrap()
}

#[test]
fn capped_direct_play_request_with_a_chosen_sub_keeps_the_external_profiles() {
    for sub in [Some(4), Some(0), Some(-1), None] {
        let body = capture_playback_body(sub, 8000);
        assert_eq!(body["EnableDirectPlay"], true, "{sub:?}");
        let methods: Vec<_> =
            body["DeviceProfile"]["SubtitleProfiles"].as_array().unwrap().iter().map(|p| p["Method"].clone()).collect();
        assert!(!methods.is_empty() && methods.iter().all(|m| m == "External"), "{sub:?}: {methods:?}");
    }
}
