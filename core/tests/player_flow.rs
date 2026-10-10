//! P2 player rules: the `Reporter` reporting state machine (R-PLAY-2) and a live progress round trip against
//! the throwaway server. Unit tests run everywhere; the live test skips without `JF_TEST_URL`.
use jf_core::api::*;
use jf_core::platform::{NullPlayer, Player, PlayerState};
use jf_core::ui::player::{Report, Reporter};

// ---- Reporter unit tests ----

#[test]
fn reporter_start_then_ten_second_cadence() {
    let mut r = Reporter::new();
    assert_eq!(r.step(0.0, &PlayerState::Loading, 0.0, false), Report::None);
    assert_eq!(r.step(1.0, &PlayerState::Playing, 0.0, false), Report::Start);
    assert_eq!(r.step(2.0, &PlayerState::Playing, 1.0, false), Report::None);
    assert_eq!(r.step(9.9, &PlayerState::Playing, 8.9, false), Report::None);
    assert_eq!(r.step(11.0, &PlayerState::Playing, 10.0, false), Report::Progress);
    assert_eq!(r.step(12.0, &PlayerState::Playing, 11.0, false), Report::None);
    assert_eq!(r.step(21.0, &PlayerState::Playing, 20.0, false), Report::Progress);
}

#[test]
fn reporter_reports_on_pause_and_unpause() {
    let mut r = Reporter::new();
    assert_eq!(r.step(0.0, &PlayerState::Playing, 0.0, false), Report::Start);
    assert_eq!(r.step(2.0, &PlayerState::Paused, 2.0, true), Report::Progress);
    assert_eq!(r.step(3.0, &PlayerState::Paused, 2.0, true), Report::None);
    assert_eq!(r.step(4.0, &PlayerState::Playing, 2.0, false), Report::Progress);
    assert_eq!(r.step(5.0, &PlayerState::Playing, 3.0, false), Report::None);
}

#[test]
fn reporter_stops_once() {
    let mut r = Reporter::new();
    assert_eq!(r.step(0.0, &PlayerState::Playing, 0.0, false), Report::Start);
    assert_eq!(r.step(5.0, &PlayerState::Ended, 5.0, false), Report::Stopped);
    assert_eq!(r.step(6.0, &PlayerState::Ended, 5.0, false), Report::None);
}

#[test]
fn reporter_ignores_pause_before_start() {
    let mut r = Reporter::new();
    assert_eq!(r.step(0.0, &PlayerState::Paused, 0.0, true), Report::None);
    assert_eq!(r.step(1.0, &PlayerState::Playing, 0.0, false), Report::Start);
}

// ---- Live test (skipped without JF_TEST_URL) ----

fn url() -> Option<String> {
    let u = std::env::var("JF_TEST_URL").ok();
    if u.is_none() {
        eprintln!("JF_TEST_URL unset: live test skipped");
    }
    u
}
fn dev() -> DeviceInfo {
    DeviceInfo { name: "live-test".into(), id: "live-test-device".into(), version: "0.1.0".into() }
}
fn signed(base: &str) -> Client {
    let (mut c, _) = Client::connect(base, dev()).unwrap();
    c.sign_in("tester", "test-pass-1").unwrap();
    c
}
fn series(c: &Client) -> Item {
    let shows = c.views().unwrap().into_iter().find(|v| v.name == "Shows").unwrap();
    c.library_page(&shows.id, 0, 10).unwrap().items.remove(0)
}
fn episode(c: &Client, s: i32, e: i32) -> Item {
    let ser = series(c);
    c.episodes(&ser.id, None)
        .unwrap()
        .into_iter()
        .find(|x| x.parent_index_number == Some(s) && x.index_number == Some(e))
        .unwrap()
}

// R-PLAY-2: Reporter decisions driven through a NullPlayer, sent with the real client; the position lands on the
// server, then is reset to 0 so later runs start clean.
#[test]
fn reporter_driven_progress_lands_on_server() {
    let Some(u) = url() else { return };
    let c = signed(&u);
    let ep = episode(&c, 2, 1);
    let pi = c.playback_info(&ep.id).unwrap();
    let base = ProgressReport {
        item_id: ep.id.clone(),
        media_source_id: pi.media_sources[0].id.clone(),
        play_session_id: pi.play_session_id.clone(),
        audio_stream_index: Some(3),
        subtitle_stream_index: None,
        ..Default::default()
    };

    let mut player = NullPlayer::default();
    let mut rep = Reporter::new();
    let mut now = 0.0;

    player.state = PlayerState::Playing;
    player.pos = 0.0;
    assert_eq!(rep.step(now, &player.state, player.position(), false), Report::Start);
    c.report_start(&base).unwrap();

    now = 8.0;
    player.state = PlayerState::Paused;
    player.pos = 8.0;
    assert_eq!(rep.step(now, &player.state, player.position(), true), Report::Progress);
    let mut r = base.clone();
    r.position_ticks = secs_to_ticks(8.0);
    r.is_paused = true;
    c.report_progress(&r).unwrap();

    player.state = PlayerState::Ended;
    assert_eq!(rep.step(now, &player.state, player.position(), false), Report::Stopped);
    c.report_stopped(&r).unwrap();

    let ud = c.item(&ep.id).unwrap().user_data.unwrap();
    assert!((ticks_to_secs(ud.playback_position_ticks) - 8.0).abs() < 1.0, "{ud:?}");

    r.position_ticks = 0;
    r.is_paused = false;
    c.report_stopped(&r).unwrap();
}
