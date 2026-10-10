//! G12 settings model: date format, Home-section order/hide, and the serde defaults that keep old
//! settings.json files loading (P-USER-*, P-HOME-5). Pure, no network.
use jf_core::platform::Storage;
use jf_core::settings::*;

#[test]
fn date_format_renders_the_leading_iso_date() {
    let s = "1967-01-01T00:00:00.0000000Z";
    assert_eq!(DateFormat::Iso.format(s), "1967-01-01");
    assert_eq!(DateFormat::Us.format(s), "01/01/1967");
    assert_eq!(DateFormat::Long.format(s), "01 Jan 1967");
}

#[test]
fn date_format_leaves_non_iso_input_alone() {
    assert_eq!(DateFormat::Iso.format("hello"), "hello");
    assert_eq!(DateFormat::Us.format(""), "");
    assert_eq!(DateFormat::Long.format("1967/01/01"), "1967/01/01");
}

#[test]
fn date_format_month_names() {
    assert_eq!(DateFormat::Long.format("2026-10-10"), "10 Oct 2026");
    assert_eq!(DateFormat::Long.format("2026-12-25"), "25 Dec 2026");
}

#[test]
fn default_settings_hold_g12_defaults() {
    let s = Settings::default();
    assert!(s.show_release_year && s.show_story_year && s.backdrops && s.auto_play_next);
    assert_eq!(s.date_format, DateFormat::Iso);
    assert_eq!(s.max_streaming_bitrate, 0);
    assert_eq!((s.skip_back_seconds, s.skip_forward_seconds), (10, 30));
    assert!(s.preferred_audio_language.is_empty());
    assert_eq!(s.home_order, HomeSection::default_order());
    assert!(s.home_hidden.is_empty());
    assert_eq!(HomeSection::default_order().len(), HomeSection::ALL.len());
}

#[test]
fn home_visible_respects_the_hidden_list() {
    let mut s = Settings::default();
    assert!(s.home_visible(HomeSection::Continue));
    s.home_hidden.push(HomeSection::Continue);
    assert!(!s.home_visible(HomeSection::Continue));
    assert!(s.home_visible(HomeSection::Libraries));
}

#[test]
fn old_settings_file_gets_the_new_defaults() {
    // A settings.json written before G12 has none of the new fields.
    let old = br#"{"device_id":"0123456789abcdef0123456789abcdef","servers":[],"last_server":null,"tmdb_key":""}"#;
    let st = MemStorage::default();
    st.write(SETTINGS_KEY, old).unwrap();
    let s = Settings::load(&st);
    assert_eq!(s.device_id, "0123456789abcdef0123456789abcdef");
    assert!(s.show_release_year && s.show_story_year && s.backdrops && s.auto_play_next);
    assert_eq!((s.skip_back_seconds, s.skip_forward_seconds), (10, 30));
    assert_eq!(s.home_order, HomeSection::default_order());
}

#[test]
fn settings_round_trip_preserves_user_prefs() {
    let st = MemStorage::default();
    let mut s = Settings::load(&st);
    s.show_release_year = false;
    s.date_format = DateFormat::Us;
    s.max_streaming_bitrate = 5000;
    s.skip_forward_seconds = 60;
    s.home_hidden.push(HomeSection::NextUp);
    let mut order = HomeSection::default_order();
    order.swap(0, 1);
    s.home_order = order;
    s.save(&st).unwrap();
    let back = Settings::load(&st);
    assert_eq!(back, s);
    assert!(!back.show_release_year);
    assert_eq!(back.date_format, DateFormat::Us);
    assert_eq!(back.max_streaming_bitrate, 5000);
    assert_eq!(back.skip_forward_seconds, 60);
    assert!(!back.home_visible(HomeSection::NextUp));
    assert_eq!(back.home_order[0], HomeSection::NextUp);
}
