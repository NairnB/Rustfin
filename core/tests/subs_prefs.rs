//! G5 / P-USER-5: the subtitle preference choice rules in `playreq` (apply_sub_prefs / build_with_prefs).
//! Uses the playbackinfo fixture: external German sub (index 0), embedded English sub (index 4); default
//! subtitle stream = 0; no forced streams.
use jf_core::api::*;
use jf_core::platform::SubStyle;
use jf_core::playreq::{apply_sub_prefs, build_with_prefs};
use jf_core::settings::{SubMode, SubPrefs};
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

fn prefs(mode: SubMode, lang: &str) -> SubPrefs {
    SubPrefs { mode, language: lang.to_string(), style: SubStyle::default() }
}

fn source() -> PlaybackInfo {
    fixture("playbackinfo.json")
}

#[test]
fn default_mode_uses_the_source_default() {
    let info = source();
    let ms = &info.media_sources[0];
    assert_eq!(apply_sub_prefs(&prefs(SubMode::Default, "eng"), ms), ms.default_subtitle_stream_index);
}

#[test]
fn none_mode_is_off() {
    let info = source();
    let ms = &info.media_sources[0];
    assert_eq!(apply_sub_prefs(&prefs(SubMode::None, "eng"), ms), None);
}

#[test]
fn always_picks_the_language_else_first() {
    let info = source();
    let ms = &info.media_sources[0];
    // Preferred "eng" → the embedded English stream (index 4).
    assert_eq!(apply_sub_prefs(&prefs(SubMode::Always, "eng"), ms), Some(4));
    // Preferred "ger" → the external German stream (index 0).
    assert_eq!(apply_sub_prefs(&prefs(SubMode::Always, "ger"), ms), Some(0));
    // No language → the first subtitle stream (index 0).
    assert_eq!(apply_sub_prefs(&prefs(SubMode::Always, ""), ms), Some(0));
    // Language with no match → the first subtitle stream.
    assert_eq!(apply_sub_prefs(&prefs(SubMode::Always, "fre"), ms), Some(0));
}

#[test]
fn only_forced_picks_forced_only() {
    let mut info = source();
    let ms = &mut info.media_sources[0];
    // Nothing is forced → off.
    assert_eq!(apply_sub_prefs(&prefs(SubMode::OnlyForced, ""), ms), None);
    // Force the embedded English stream → it is picked.
    ms.media_streams.iter_mut().find(|s| s.index == 4).unwrap().is_forced = true;
    assert_eq!(apply_sub_prefs(&prefs(SubMode::OnlyForced, ""), ms), Some(4));
}

#[test]
fn the_item_pages_choice_wins_over_prefs() {
    let item: Item = fixture("episode_item.json");
    let info = source();
    // The item page chose "off" → prefs must not override it.
    let off = PlayOptions { subtitle: Some(None), ..Default::default() };
    let r = build_with_prefs(&client(), &item, &info, &off, &prefs(SubMode::Always, "eng")).unwrap();
    assert_eq!(r.subtitle, None);
}

#[test]
fn prefs_apply_when_the_item_page_did_not_choose() {
    let item: Item = fixture("episode_item.json");
    let info = source();
    let r = build_with_prefs(&client(), &item, &info, &PlayOptions::default(), &prefs(SubMode::Always, "eng")).unwrap();
    assert_eq!(r.subtitle, Some(4));
}
