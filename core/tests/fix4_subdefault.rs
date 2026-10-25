//! FIX-4 (24 Oct): default subtitle — off unless the audio isn't English (or the viewer picks one); the file's
//! default flag is ignored.
use jf_core::api::*;
use jf_core::playreq::{build_with_prefs, default_subtitle};
use jf_core::settings::{SubMode, SubPick, SubPrefs};
use jf_core::ui::player::PlayOptions;

fn client() -> Client {
    let d = DeviceInfo { name: "t".into(), id: "d".into(), version: "0".into() };
    Client::with_session(
        "http://h:8096",
        d,
        Session { user_id: "u".into(), user_name: "t".into(), token: "tok".into() },
    )
}

fn prefs(mode: SubMode, lang: &str) -> SubPrefs {
    SubPrefs { mode, language: lang.into(), ..Default::default() }
}

// 0 ger sub (external), 2 eng audio, 3 fre audio, 4 eng sub; no server default (a pick it remembered is its own test).
fn info() -> PlaybackInfo {
    let mut i: PlaybackInfo = serde_json::from_str(
        &std::fs::read_to_string(format!("{}/tests/fixtures/playbackinfo.json", env!("CARGO_MANIFEST_DIR"))).unwrap(),
    )
    .unwrap();
    i.media_sources[0].default_subtitle_stream_index = None;
    i
}

fn set_lang(ms: &mut MediaSource, index: i32, lang: Option<&str>) {
    ms.media_streams.iter_mut().find(|s| s.index == index).unwrap().language = lang.map(String::from);
}

fn item() -> Item {
    serde_json::from_str(r#"{"Id":"i","Name":"Film","Type":"Movie"}"#).unwrap()
}

#[test]
fn r1_english_audio_is_off_whatever_the_file_flags() {
    let mut i = info();
    let ms = &mut i.media_sources[0];
    ms.default_subtitle_stream_index = Some(0);
    ms.media_streams.iter_mut().for_each(|s| {
        s.is_default = s.kind == StreamKind::Subtitle;
        s.is_forced = s.index == 4;
    });
    let p = prefs(SubMode::Default, "");
    assert_eq!(default_subtitle(&p, ms, None, None), None);
    assert_eq!(default_subtitle(&p, ms, Some(2), None), None);
    // unknown audio language counts as English
    set_lang(ms, 2, None);
    assert_eq!(default_subtitle(&p, ms, Some(2), None), None);
    set_lang(ms, 2, Some("und"));
    assert_eq!(default_subtitle(&p, ms, Some(2), None), None);
    set_lang(ms, 2, Some("en"));
    assert_eq!(default_subtitle(&p, ms, Some(2), None), None);
}

#[test]
fn r2_other_audio_gets_the_english_sub() {
    let i = info();
    let ms = &i.media_sources[0];
    let p = prefs(SubMode::Default, "");
    assert_eq!(default_subtitle(&p, ms, Some(3), None), Some(4), "fre audio → the eng sub");
    // no English sub: the preferred language, else Off
    let mut i = info();
    let ms = &mut i.media_sources[0];
    set_lang(ms, 4, Some("spa"));
    assert_eq!(default_subtitle(&prefs(SubMode::Default, "ger"), ms, Some(3), None), Some(0));
    assert_eq!(default_subtitle(&p, ms, Some(3), None), None);
    // the source's default audio decides when none is named
    ms.default_audio_stream_index = Some(3);
    assert_eq!(default_subtitle(&prefs(SubMode::Default, "ger"), ms, None, None), Some(0));
}

#[test]
fn r2_german_audio_film_gets_english() {
    let mut i = info();
    let ms = &mut i.media_sources[0];
    set_lang(ms, 2, Some("ger"));
    ms.default_audio_stream_index = Some(2);
    assert_eq!(default_subtitle(&prefs(SubMode::Default, ""), ms, None, None), Some(4));
}

#[test]
fn r3_a_viewer_pick_wins() {
    let i = info();
    let pick = PlayOptions { subtitle: Some(Some(0)), ..Default::default() };
    let req = build_with_prefs(&client(), &item(), &i, &pick, &prefs(SubMode::Default, "")).unwrap();
    assert_eq!(req.subtitle, Some(0));
    let off = PlayOptions { subtitle: Some(None), audio: Some(3), ..Default::default() };
    let req = build_with_prefs(&client(), &item(), &i, &off, &prefs(SubMode::Default, "")).unwrap();
    assert_eq!(req.subtitle, None);
}

#[test]
fn player_follows_the_chosen_audio() {
    let i = info();
    let fre = PlayOptions { audio: Some(3), ..Default::default() };
    let req = build_with_prefs(&client(), &item(), &i, &fre, &prefs(SubMode::Default, "")).unwrap();
    assert_eq!(req.subtitle, Some(4));
    let req = build_with_prefs(&client(), &item(), &i, &PlayOptions::default(), &prefs(SubMode::Default, "")).unwrap();
    assert_eq!(req.subtitle, None);
}

#[test]
fn r4_other_modes_unchanged() {
    let i = info();
    let ms = &i.media_sources[0];
    assert_eq!(default_subtitle(&prefs(SubMode::None, "eng"), ms, Some(3), None), None);
    assert_eq!(default_subtitle(&prefs(SubMode::Always, "eng"), ms, Some(2), None), Some(4));
    assert_eq!(default_subtitle(&prefs(SubMode::Always, ""), ms, Some(2), None), Some(0));
}

#[test]
fn r2_forced_english_beats_full_english() {
    let mut i = info();
    let ms = &mut i.media_sources[0];
    let mut extra = ms.media_streams.iter().find(|s| s.index == 4).unwrap().clone();
    extra.index = 9;
    extra.is_forced = true;
    ms.media_streams.push(extra);
    // 4 = full English, 9 = English forced: the forced one comes on for foreign audio; with none, the full one
    assert_eq!(default_subtitle(&prefs(SubMode::Default, ""), ms, Some(3), None), Some(9));
    ms.media_streams.retain(|s| s.index != 9);
    assert_eq!(default_subtitle(&prefs(SubMode::Default, ""), ms, Some(3), None), Some(4));
}

fn pick(index: i32, lang: Option<&str>, forced: bool) -> SubPick {
    SubPick { index, language: lang.map(String::from), forced }
}

#[test]
fn the_remembered_pick_is_the_viewers_own_not_the_servers_default() {
    let mut i = info();
    let ms = &mut i.media_sources[0];
    let p = prefs(SubMode::Default, "");
    // the server's DefaultSubtitleStreamIndex (what it picks itself, e.g. the first sidecar) is ignored, flagged or not
    ms.default_subtitle_stream_index = Some(0);
    ms.media_streams.iter_mut().for_each(|s| {
        s.is_default = false;
        s.is_forced = false;
    });
    assert_eq!(default_subtitle(&p, ms, Some(2), None), None);
    assert_eq!(default_subtitle(&p, ms, Some(3), None), Some(4));
    // the viewer's remembered pick wins, whatever the audio
    let ger = pick(0, Some("ger"), false);
    assert_eq!(default_subtitle(&p, ms, Some(2), Some(&ger)), Some(0));
    assert_eq!(default_subtitle(&p, ms, Some(3), Some(&ger)), Some(0));
    // -1 = the viewer chose Off: Off even for foreign audio
    assert_eq!(default_subtitle(&p, ms, Some(3), Some(&pick(-1, None, false))), None);
    // a remembered stream that no longer exists (or is not a subtitle: 2 is audio) falls back to the default-subtitle rule
    assert_eq!(default_subtitle(&p, ms, Some(3), Some(&pick(77, None, false))), Some(4));
    assert_eq!(default_subtitle(&p, ms, Some(2), Some(&pick(2, None, false))), None);
    // another episode of the series: the same language (and forced flag) at another index
    assert_eq!(default_subtitle(&p, ms, Some(2), Some(&pick(9, Some("ger"), false))), Some(0));
    assert_eq!(default_subtitle(&p, ms, Some(2), Some(&pick(0, Some("eng"), false))), Some(4), "index 0 is German now");
    assert_eq!(default_subtitle(&p, ms, Some(2), Some(&pick(9, Some("ger"), true))), None, "no German forced track");
    // other modes ignore it
    assert_eq!(default_subtitle(&prefs(SubMode::None, ""), ms, Some(2), Some(&ger)), None);
    assert_eq!(default_subtitle(&prefs(SubMode::Always, "eng"), ms, Some(2), Some(&ger)), Some(4));
}

#[test]
fn the_play_request_uses_the_remembered_pick_it_is_given() {
    let info = info();
    let p = prefs(SubMode::Default, "");
    let r = build_with_prefs(&client(), &item(), &info, &PlayOptions::default(), &p).unwrap();
    assert_eq!(r.subtitle, None, "nothing remembered: eng audio → Off");
    let ger = PlayOptions { remembered_subtitle: Some(pick(0, Some("ger"), false)), ..Default::default() };
    assert_eq!(build_with_prefs(&client(), &item(), &info, &ger, &p).unwrap().subtitle, Some(0));
    let off = PlayOptions { audio: Some(3), remembered_subtitle: Some(pick(-1, None, false)), ..Default::default() };
    assert_eq!(build_with_prefs(&client(), &item(), &info, &off, &p).unwrap().subtitle, None);
}

#[test]
fn r5_audio_pick_reapplies_the_default_unless_the_viewer_picked() {
    use jf_core::playreq::subtitle_after_audio_pick as after;
    let i = info();
    let ms = &i.media_sources[0];
    let p = prefs(SubMode::Default, "");
    // eng audio showed Off; the viewer switches to fre audio → the English sub comes on; and back → Off again
    assert_eq!(after(&p, ms, 3, None, false, Some(None)), Some(Some(4)));
    assert_eq!(after(&p, ms, 2, None, false, Some(Some(4))), Some(None));
    // a subtitle the viewer picked stays through an audio change
    assert_eq!(after(&p, ms, 3, None, true, Some(None)), Some(None));
    assert_eq!(after(&p, ms, 2, None, true, Some(Some(0))), Some(Some(0)));
    // the remembered pick is applied too
    assert_eq!(after(&p, ms, 3, Some(&pick(0, Some("ger"), false)), false, Some(None)), Some(Some(0)));
}

#[test]
fn tv_burns_in_the_english_sub_for_foreign_audio_and_nothing_for_english() {
    use jf_core::playreq::build_plan_burnt_on;
    let plan = PlaybackPlan {
        info: info(),
        transcoding_url: Some("http://h:8096/v/master.m3u8?api_key=tok".into()),
        source_bitrate: None,
    };
    let p = prefs(SubMode::Default, "");
    let fre = PlayOptions { audio: Some(3), ..Default::default() };
    let (r, burnt) = build_plan_burnt_on(false, &client(), &item(), &plan, &fre, &p).unwrap();
    assert_eq!(burnt, Some(4));
    assert!(r.url.contains("SubtitleStreamIndex=4") && r.url.contains("SubtitleMethod=Encode"), "{}", r.url);
    let eng = PlayOptions { audio: Some(2), ..Default::default() };
    let (r, burnt) = build_plan_burnt_on(false, &client(), &item(), &plan, &eng, &p).unwrap();
    assert_eq!(burnt, None);
    assert!(!r.url.contains("SubtitleMethod=Encode"), "{}", r.url);
}
