//! P1 rules for the play request (R-PLAY-1, R-PLAY-3). Lead's tests; builders never edit.
use jf_core::api::*;
use jf_core::playreq::build;
use jf_core::ui::player::PlayOptions;

fn fixture<T: serde::de::DeserializeOwned>(n: &str) -> T {
    serde_json::from_str(
        &std::fs::read_to_string(format!("{}/tests/fixtures/{n}", env!("CARGO_MANIFEST_DIR"))).unwrap(),
    )
    .unwrap()
}
fn client() -> Client {
    let d = DeviceInfo { name: "t".into(), id: "d".into(), version: "0".into() };
    Client::with_session(
        "http://h:8096",
        d,
        Session { user_id: "u".into(), user_name: "t".into(), token: "tok".into() },
    )
}

#[test]
fn episode_request_defaults() {
    let mut item: Item = fixture("episode_item.json");
    item.user_data.as_mut().unwrap().playback_position_ticks = 75_000_000;
    let info: PlaybackInfo = fixture("playbackinfo.json");
    let ms = &info.media_sources[0];
    let r = build(&client(), &item, &info, &PlayOptions::default()).unwrap();
    assert_eq!(
        r.url,
        format!("http://h:8096/Videos/{}/stream?static=true&mediaSourceId={}&api_key=tok", item.id, ms.id)
    );
    assert_eq!(r.start_secs, 7.5);
    assert_eq!(r.audio_indexes, vec![2, 3]);
    assert_eq!(r.subtitle_indexes, vec![4]);
    assert_eq!(r.external_subs.len(), 1);
    assert_eq!(r.external_subs[0].index, 0);
    assert!(r.external_subs[0].url.ends_with("/Subtitles/0/0/Stream.srt?api_key=tok"));
    assert_eq!(r.audio, ms.default_audio_stream_index.or(Some(2)));
    // FIX-4: the server's DefaultSubtitleStreamIndex is ignored; eng audio and no remembered pick → Off
    assert_eq!(ms.default_subtitle_stream_index, Some(0));
    assert_eq!(r.subtitle, None);
    assert!(r.title.starts_with("Night Shift Stories · S1E"), "{}", r.title);
}

#[test]
fn choices_win() {
    let item: Item = fixture("episode_item.json");
    let info: PlaybackInfo = fixture("playbackinfo.json");
    let o = PlayOptions { from_start: true, audio: Some(3), subtitle: Some(Some(0)), ..Default::default() };
    let r = build(&client(), &item, &info, &o).unwrap();
    assert_eq!((r.start_secs, r.audio, r.subtitle), (0.0, Some(3), Some(0)));
    let off = PlayOptions { subtitle: Some(None), ..o };
    assert_eq!(build(&client(), &item, &info, &off).unwrap().subtitle, None);
}

#[test]
fn no_source_is_none() {
    let item: Item = fixture("episode_item.json");
    assert!(build(&client(), &item, &PlaybackInfo::default(), &PlayOptions::default()).is_none());
}
