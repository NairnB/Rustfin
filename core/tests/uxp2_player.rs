//! UXP2: the player built for a remote — OSD key model (D4, R1), readable track names (D14), subtitle lift,
//! and the Info panel rows. Pure functions, no player or server.
use jf_core::api::{MediaStream, StreamKind};
use jf_core::platform::Key;
use jf_core::ui::player::{
    lifted_sub_position, now_playing_rows, osd_key, track_labels, OsdAct, OsdFocus, OSD_SUB_LIFT,
};

fn st(index: i32, kind: StreamKind, lang: Option<&str>) -> MediaStream {
    MediaStream { index, kind, language: lang.map(String::from), ..Default::default() }
}

// ---- OSD key model ----

#[test]
fn hidden_left_right_seek_by_the_skip_sizes() {
    assert_eq!(osd_key(false, OsdFocus::Timeline, Key::Left, 10.0, 30.0), OsdAct::Seek(-10.0));
    assert_eq!(osd_key(false, OsdFocus::Other, Key::Right, 10.0, 30.0), OsdAct::Seek(30.0));
}

#[test]
fn hidden_up_down_enter_wake_the_osd() {
    for k in [Key::Up, Key::Down, Key::Enter] {
        assert_eq!(osd_key(false, OsdFocus::Timeline, k, 10.0, 30.0), OsdAct::Wake, "{k:?}");
    }
}

#[test]
fn hidden_back_leaves_the_player() {
    assert_eq!(osd_key(false, OsdFocus::Timeline, Key::Back, 10.0, 30.0), OsdAct::Leave);
}

#[test]
fn shown_back_hides_the_osd_first() {
    assert_eq!(osd_key(true, OsdFocus::Timeline, Key::Back, 10.0, 30.0), OsdAct::HideOsd);
    assert_eq!(osd_key(true, OsdFocus::Other, Key::Back, 10.0, 30.0), OsdAct::HideOsd);
}

#[test]
fn shown_timeline_left_right_scrub() {
    assert_eq!(osd_key(true, OsdFocus::Timeline, Key::Left, 10.0, 30.0), OsdAct::Seek(-10.0));
    assert_eq!(osd_key(true, OsdFocus::Timeline, Key::Right, 10.0, 30.0), OsdAct::Seek(30.0));
}

#[test]
fn shown_down_moves_to_the_buttons_and_left_right_walk_them() {
    assert_eq!(osd_key(true, OsdFocus::Timeline, Key::Down, 10.0, 30.0), OsdAct::Move);
    assert_eq!(osd_key(true, OsdFocus::Other, Key::Left, 10.0, 30.0), OsdAct::Move);
    assert_eq!(osd_key(true, OsdFocus::Other, Key::Right, 10.0, 30.0), OsdAct::Move);
    assert_eq!(osd_key(true, OsdFocus::Other, Key::Enter, 10.0, 30.0), OsdAct::Activate);
}

#[test]
fn shown_volume_bar_keys_change_the_volume() {
    assert_eq!(osd_key(true, OsdFocus::VolumeBar, Key::Left, 10.0, 30.0), OsdAct::Volume(-0.05));
    assert_eq!(osd_key(true, OsdFocus::VolumeBar, Key::Up, 10.0, 30.0), OsdAct::Volume(0.05));
}

// ---- track names ----

#[test]
fn a_plain_language_reads_as_its_name() {
    let v = [st(1, StreamKind::Subtitle, Some("ger")), st(2, StreamKind::Subtitle, Some("eng"))];
    assert_eq!(track_labels(&v), ["German", "English"]);
}

#[test]
fn two_of_one_language_say_embedded_and_external() {
    let mut ext = st(5, StreamKind::Subtitle, Some("eng"));
    ext.is_external = true;
    let v = [st(3, StreamKind::Subtitle, Some("eng")), ext];
    assert_eq!(track_labels(&v), ["English (embedded)", "English (external)"]);
}

#[test]
fn forced_tracks_say_so() {
    let mut forced = st(4, StreamKind::Subtitle, Some("eng"));
    forced.is_forced = true;
    let v = [st(3, StreamKind::Subtitle, Some("eng")), forced];
    assert_eq!(track_labels(&v), ["English", "English — Forced"]);
}

#[test]
fn jargon_display_titles_are_not_used() {
    let mut s = st(2, StreamKind::Subtitle, Some("ger"));
    s.display_title = Some("Ger - SUBRIP - External".into());
    s.is_external = true;
    assert_eq!(track_labels(&[s]), ["German"]);
}

#[test]
fn unknown_language_falls_back_to_the_title_then_a_number() {
    let mut a = st(2, StreamKind::Audio, None);
    a.title = Some("Director's commentary".into());
    let b = st(3, StreamKind::Audio, None);
    assert_eq!(track_labels(&[a, b]), ["Director's commentary", "Track 3"]);
}

#[test]
fn identical_labels_are_numbered() {
    let v = [st(1, StreamKind::Audio, Some("eng")), st(2, StreamKind::Audio, Some("eng"))];
    assert_eq!(track_labels(&v), ["English 1", "English 2"]);
}

// ---- subtitles above the OSD ----

#[test]
fn subtitles_lift_only_while_the_osd_is_up() {
    assert_eq!(lifted_sub_position(100.0, false), 100.0);
    assert_eq!(lifted_sub_position(100.0, true), 100.0 - OSD_SUB_LIFT);
    assert_eq!(lifted_sub_position(10.0, true), 10.0, "never pushed past a position already above the lift");
}

// ---- Info ----

#[test]
fn info_rows_show_what_is_playing() {
    let rows = now_playing_rows("Show S1E2", 65.0, 3600.0, "English", "German");
    assert_eq!(
        rows,
        vec![
            ("Playing", "Show S1E2".to_string()),
            ("Time", "1:05 / 1:00:00".to_string()),
            ("Audio", "English".to_string()),
            ("Subtitles", "German".to_string()),
        ]
    );
}
