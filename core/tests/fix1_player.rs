//! FIX-1 #3 / #16 (player): what the player says about what's playing, and how Down wakes the controls.
use jf_core::platform::Key;
use jf_core::ui::player::{now_playing_line, osd_key, OsdAct, OsdFocus, OSD_BUTTON_HOLD_SECS, OSD_START_SECS};

#[test]
fn hidden_down_wakes_onto_the_buttons() {
    assert_eq!(osd_key(false, OsdFocus::Timeline, Key::Down, 10.0, 30.0), OsdAct::WakeButtons);
}

#[test]
fn hidden_up_and_enter_wake_onto_the_timeline() {
    for k in [Key::Up, Key::Enter] {
        assert_eq!(osd_key(false, OsdFocus::Timeline, k, 10.0, 30.0), OsdAct::Wake, "{k:?}");
    }
}

#[test]
#[allow(clippy::assertions_on_constants)]
fn start_shows_the_osd_a_while_and_buttons_hold_it_longer() {
    assert!(OSD_START_SECS >= 5.0);
    assert!(OSD_BUTTON_HOLD_SECS > OSD_START_SECS);
}

#[test]
fn now_playing_names_audio_and_subtitles_when_on() {
    assert_eq!(now_playing_line("English — 5.1 Surround", None), "English — 5.1 Surround");
    assert_eq!(now_playing_line("English — Stereo", Some("German")), "English — Stereo · German subtitles");
}

#[test]
fn keys_during_the_start_osd_act_as_if_hidden() {
    // The player passes visible = false while the OSD is only up because playback started (osd_auto),
    // so Back still leaves and Left/Right still seek, as before FIX-1 #3.
    assert_eq!(osd_key(false, OsdFocus::Timeline, Key::Back, 10.0, 30.0), OsdAct::Leave);
    assert_eq!(osd_key(false, OsdFocus::Timeline, Key::Right, 10.0, 30.0), OsdAct::Seek(30.0));
}

#[test]
fn subtitles_list_ends_with_the_options_row() {
    use jf_core::ui::player::{sheet_row, SheetRow};
    // "Off" + 2 tracks, then "Subtitle options…".
    assert_eq!(sheet_row(0, 3), Some(SheetRow::Track(0)));
    assert_eq!(sheet_row(2, 3), Some(SheetRow::Track(2)));
    assert_eq!(sheet_row(3, 3), Some(SheetRow::Options));
    assert_eq!(sheet_row(4, 3), None);
}
