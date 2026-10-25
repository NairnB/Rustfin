//! FIX-3 tv-subs rules (T1..T4): a backend that cannot render subtitles (the real TV) gets them burnt in by the server.
use jf_core::api::*;
use jf_core::platform::{NullPlayer, Player};
use jf_core::playreq::{build_plan_burnt, build_plan_burnt_on, must_transcode, request_indexes, request_indexes_on};
use jf_core::ui::player::{subtitle_change_replans, subtitle_change_replans_on, PlayOptions};

fn client() -> Client {
    let d = DeviceInfo { name: "t".into(), id: "d".into(), version: "0".into() };
    Client::with_session(
        "http://h:8096",
        d,
        Session { user_id: "u".into(), user_name: "t".into(), token: "tok".into() },
    )
}

// playbackinfo.json: 0 = external ger sub, 2 = eng audio, 3 = fre audio, 4 = embedded eng sub.
fn info() -> PlaybackInfo {
    serde_json::from_str(
        &std::fs::read_to_string(format!("{}/tests/fixtures/playbackinfo.json", env!("CARGO_MANIFEST_DIR"))).unwrap(),
    )
    .unwrap()
}

fn plan(url: Option<&str>) -> PlaybackPlan {
    PlaybackPlan { info: info(), transcoding_url: url.map(String::from), source_bitrate: None }
}

fn opts(subtitle: Option<Option<i32>>) -> PlayOptions {
    PlayOptions { audio: Some(2), subtitle, ..Default::default() }
}

#[test]
fn tv_backend_with_subtitle_forces_encode() {
    // any chosen subtitle forces the transcode path
    assert!(must_transcode(false, Some(4)));
    assert!(must_transcode(false, Some(0)));
    // the request names the index, an external one too (the TV cannot load it itself)
    let i = info();
    assert_eq!(request_indexes_on(false, Some(&i), None, Some(2), Some(Some(4))), (Some(2), Some(4)));
    assert_eq!(request_indexes_on(false, Some(&i), None, Some(2), Some(Some(0))), (Some(2), Some(0)));
    // and the stream url burns it in, embedded or external
    let p = plan(Some("http://h:8096/v/master.m3u8?api_key=tok"));
    for s in [4, 0] {
        let (r, burnt) =
            build_plan_burnt_on(false, &client(), &Item::default(), &p, &opts(Some(Some(s))), &Default::default())
                .unwrap();
        assert_eq!(burnt, Some(s));
        assert!(
            r.url.contains(&format!("SubtitleStreamIndex={s}")) && r.url.contains("SubtitleMethod=Encode"),
            "{}",
            r.url
        );
        assert_eq!(r.subtitle, None, "nothing for the player to load");
    }
}

#[test]
fn tv_backend_off_stays_direct() {
    assert!(!must_transcode(false, None));
    let i = info();
    assert_eq!(request_indexes_on(false, Some(&i), None, Some(2), Some(None)), (Some(2), Some(-1)));
    // a direct plan is untouched
    let p = plan(None);
    let a = build_plan_burnt_on(false, &client(), &Item::default(), &p, &opts(Some(None)), &Default::default());
    let b = build_plan_burnt(&client(), &Item::default(), &p, &opts(Some(None)), &Default::default());
    assert_eq!(a, b);
    // a transcode with Off carries no Encode
    let t = plan(Some("http://h:8096/v/master.m3u8?api_key=tok"));
    let (r, burnt) =
        build_plan_burnt_on(false, &client(), &Item::default(), &t, &opts(Some(None)), &Default::default()).unwrap();
    assert_eq!(burnt, None);
    assert!(r.url.contains("SubtitleStreamIndex=-1") && !r.url.contains("Encode"), "{}", r.url);
}

#[test]
fn tv_subtitle_change_reloads_at_position() {
    // direct play on the TV: choosing a subtitle re-plans (reload keeps the current position), as does moving
    // between two subtitles, or switching back to Off while the stream is a subtitle transcode
    assert!(subtitle_change_replans_on(false, false, Some(-1), Some(4)));
    assert!(subtitle_change_replans_on(false, true, Some(4), Some(0)));
    assert!(subtitle_change_replans_on(false, true, Some(4), Some(-1)));
    // off → off changes nothing
    assert!(!subtitle_change_replans_on(false, false, Some(-1), Some(-1)));
    assert!(!subtitle_change_replans_on(false, true, Some(-1), Some(-1)));
}

#[test]
fn desktop_backend_unchanged() {
    assert!(NullPlayer::default().renders_subtitles(), "the desktop player draws subtitles itself");
    assert!(!must_transcode(true, Some(4)) && !must_transcode(true, Some(0)));
    let i = info();
    for sub in [None, Some(None), Some(Some(4)), Some(Some(0))] {
        assert_eq!(
            request_indexes_on(true, Some(&i), None, Some(2), sub),
            request_indexes(Some(&i), None, Some(2), sub)
        );
    }
    for p in [plan(None), plan(Some("http://h:8096/v/master.m3u8?api_key=tok"))] {
        for sub in [None, Some(None), Some(Some(4)), Some(Some(0))] {
            let a = build_plan_burnt_on(true, &client(), &Item::default(), &p, &opts(sub), &Default::default());
            let b = build_plan_burnt(&client(), &Item::default(), &p, &opts(sub), &Default::default());
            assert_eq!(a, b);
        }
    }
    assert!(!subtitle_change_replans_on(true, false, Some(-1), Some(4)), "direct play switches in place");
    assert_eq!(
        subtitle_change_replans_on(true, true, Some(-1), Some(4)),
        subtitle_change_replans(true, Some(-1), Some(4))
    );
    assert_eq!(
        subtitle_change_replans_on(true, true, Some(-1), Some(-1)),
        subtitle_change_replans(true, Some(-1), Some(-1))
    );
}
