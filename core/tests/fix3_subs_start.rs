//! FIX-3 player-subs-start (S1..S4): the player starts with the subtitle the film page shows, from every way in.
use jf_core::api::*;
use jf_core::playreq::{build_plan, build_plan_burnt, build_with_prefs, default_subtitle};
use jf_core::settings::{SubMode, SubPrefs};
use jf_core::ui::player::PlayOptions;

fn client() -> Client {
    let d = DeviceInfo { name: "t".into(), id: "d".into(), version: "0".into() };
    Client::with_session(
        "http://h:8096",
        d,
        Session { user_id: "u".into(), user_name: "t".into(), token: "tok".into() },
    )
}

// playbackinfo.json: 0 = external ger sub, 4 = embedded eng sub, default sub index 0. Here: no server default,
// embedded sub 4 flagged default by the file (the Sintel case).
fn info_flagged() -> PlaybackInfo {
    let mut info: PlaybackInfo = serde_json::from_str(
        &std::fs::read_to_string(format!("{}/tests/fixtures/playbackinfo.json", env!("CARGO_MANIFEST_DIR"))).unwrap(),
    )
    .unwrap();
    let ms = &mut info.media_sources[0];
    ms.default_subtitle_stream_index = None;
    ms.media_streams.iter_mut().find(|s| s.index == 4).unwrap().is_default = true;
    info
}

fn info_unflagged() -> PlaybackInfo {
    let mut info = info_flagged();
    info.media_sources[0].media_streams.iter_mut().for_each(|s| s.is_default = false);
    info.media_sources[0].media_streams.iter_mut().for_each(|s| s.is_forced = false);
    info
}

fn prefs(mode: SubMode, lang: &str) -> SubPrefs {
    SubPrefs { mode, language: lang.into(), ..Default::default() }
}

fn item() -> Item {
    serde_json::from_str(r#"{"Id":"i","Name":"Sintel","Type":"Movie"}"#).unwrap()
}

fn plan(info: PlaybackInfo) -> PlaybackPlan {
    PlaybackPlan { info, transcoding_url: None, source_bitrate: None }
}

#[test]
fn resume_from_home_uses_page_default_subtitle() {
    // Home Resume carries no explicit pick (subtitle None): the player starts with what the page would show.
    let info = info_flagged();
    let ms = &info.media_sources[0];
    let p = prefs(SubMode::Default, "");
    // FIX-4: English audio → Off, whatever the file flags.
    assert_eq!(default_subtitle(&p, ms, None, None), None);
    let req = build_plan(&client(), &item(), &plan(info.clone()), &PlayOptions::default(), &p).unwrap();
    assert_eq!(req.subtitle, None);
    // ... and a file with nothing flagged starts with Off, never an invented track.
    let info = info_unflagged();
    let req = build_plan(&client(), &item(), &plan(info), &PlayOptions::default(), &p).unwrap();
    assert_eq!(req.subtitle, None);
}

#[test]
fn page_and_player_share_the_default() {
    for info in [info_flagged(), info_unflagged()] {
        let ms = &info.media_sources[0];
        for p in [
            prefs(SubMode::Default, ""),
            prefs(SubMode::None, "eng"),
            prefs(SubMode::Always, "ger"),
            prefs(SubMode::Always, ""),
            prefs(SubMode::OnlyForced, ""),
        ] {
            let page = default_subtitle(&p, ms, None, None);
            let req = build_with_prefs(&client(), &item(), &info, &PlayOptions::default(), &p).unwrap();
            assert_eq!(req.subtitle, page, "{:?}", p.mode);
            let (req, burnt) =
                build_plan_burnt(&client(), &item(), &plan(info.clone()), &PlayOptions::default(), &p).unwrap();
            assert_eq!(burnt, None);
            assert_eq!(req.subtitle, page, "{:?}", p.mode);
        }
    }
}

#[test]
fn off_reaches_backend_as_none_not_auto() {
    // User pref "None" beats a file-flagged default: the request says no subtitle.
    let info = info_flagged();
    let req = build_with_prefs(&client(), &item(), &info, &PlayOptions::default(), &prefs(SubMode::None, "")).unwrap();
    assert_eq!(req.subtitle, None);
    // An explicit Off (Some(None)) is also None.
    let off = PlayOptions { subtitle: Some(None), ..Default::default() };
    let req = build_with_prefs(&client(), &item(), &info, &off, &prefs(SubMode::Always, "eng")).unwrap();
    assert_eq!(req.subtitle, None);
}

#[test]
fn explicit_pick_beats_default() {
    let info = info_flagged();
    let pick = PlayOptions { subtitle: Some(Some(0)), ..Default::default() };
    let req = build_with_prefs(&client(), &item(), &info, &pick, &prefs(SubMode::Default, "")).unwrap();
    assert_eq!(req.subtitle, Some(0));
    let req = build_with_prefs(&client(), &item(), &info, &pick, &prefs(SubMode::None, "")).unwrap();
    assert_eq!(req.subtitle, Some(0));
}

#[test]
fn default_mode_ignores_the_files_flags() {
    // FIX-4: the server's -1 or none, or the flagged embedded sub 4 as its default, changes nothing — English audio
    // starts Off. (A server default naming an unflagged stream is a remembered pick: see fix4_subdefault.)
    for server in [Some(-1), Some(4), None] {
        let mut info = info_flagged();
        info.media_sources[0].default_subtitle_stream_index = server;
        assert_eq!(
            default_subtitle(&prefs(SubMode::Default, ""), &info.media_sources[0], None, None),
            None,
            "{server:?}"
        );
    }
}
