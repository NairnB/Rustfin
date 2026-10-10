//! G10 media segments (P-PLAY-11): "which segment am I in", kind mapping, settings default. No server needed.
use jf_core::api::{Segment, SegmentKind};
use jf_core::settings::Settings;
use jf_core::ui::player::active_segment;

fn seg(kind: SegmentKind, start: f64, end: f64) -> Segment {
    Segment { kind, start, end }
}

fn fixture() -> Vec<Segment> {
    vec![
        seg(SegmentKind::Recap, 0.0, 30.0),
        seg(SegmentKind::Intro, 30.0, 90.0),
        seg(SegmentKind::Unknown, 100.0, 200.0),
        seg(SegmentKind::Outro, 1300.0, 1400.0),
    ]
}

#[test]
fn inside_a_segment_finds_it() {
    let s = fixture();
    assert_eq!(active_segment(&s, 10.0), Some(0));
    assert_eq!(active_segment(&s, 45.0), Some(1));
    assert_eq!(active_segment(&s, 1350.0), Some(3));
}

#[test]
fn start_is_inside_end_is_not() {
    let s = fixture();
    assert_eq!(active_segment(&s, 30.0), Some(1));
    assert_eq!(active_segment(&s, 90.0), None);
    assert_eq!(active_segment(&s, 1400.0), None);
}

#[test]
fn between_segments_and_unknown_offer_nothing() {
    let s = fixture();
    assert_eq!(active_segment(&s, 95.0), None);
    assert_eq!(active_segment(&s, 150.0), None, "Unknown has no button");
    assert_eq!(active_segment(&[], 5.0), None);
}

#[test]
fn kinds_map_from_the_server_and_label() {
    assert_eq!(SegmentKind::from_server("Intro"), SegmentKind::Intro);
    assert_eq!(SegmentKind::from_server("Outro"), SegmentKind::Outro);
    assert_eq!(SegmentKind::from_server("Whatever"), SegmentKind::Unknown);
    assert_eq!(SegmentKind::Intro.button_label(), Some("Skip intro"));
    assert_eq!(SegmentKind::Recap.button_label(), Some("Skip recap"));
    assert_eq!(SegmentKind::Outro.button_label(), Some("Skip credits"));
    assert_eq!(SegmentKind::Unknown.button_label(), None);
}

#[test]
fn auto_skip_defaults_empty_and_round_trips() {
    assert!(Settings::default().auto_skip.is_empty());
    let s = Settings { auto_skip: vec![SegmentKind::Intro, SegmentKind::Outro], ..Default::default() };
    let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
    assert_eq!(back.auto_skip, s.auto_skip);
    // An older settings file without the key still loads.
    let old: Settings = serde_json::from_str("{}").unwrap();
    assert!(old.auto_skip.is_empty());
}
