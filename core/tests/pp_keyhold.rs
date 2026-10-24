//! PP-1 (TV): key-down state for shells that only send key-down events (Tizen web, EFL native).
use jf_core::keyhold::KeyHold;
use jf_core::platform::Key;

#[test]
fn pp_hold_remote_with_pause_then_repeats_stays_down() {
    let mut h = KeyHold::default();
    h.press(Key::Enter, 0.0);
    // the remote pauses 450 ms, then repeats every 100 ms
    assert!(h.down(0.3).contains(&Key::Enter));
    for i in 0..8 {
        let t = 0.45 + 0.1 * i as f64;
        h.press(Key::Enter, t);
        assert!(h.down(t).contains(&Key::Enter), "down at {t}");
    }
    assert_eq!(h.held_secs(Key::Enter, 1.2), Some(1.2));
    // stops repeating -> released
    assert!(!h.down(2.0).contains(&Key::Enter));
}

#[test]
fn pp_hold_single_press_is_released_before_600ms() {
    let mut h = KeyHold::default();
    h.press(Key::Enter, 1.0);
    assert!(h.down(1.3).contains(&Key::Enter));
    assert!(!h.down(1.0 + 0.6).contains(&Key::Enter), "a single tap must be up by the 0.6 s hold threshold");
}

#[test]
fn pp_hold_key_up_mode_tap_is_up_at_once_and_hold_is_down_at_600ms() {
    let mut h = KeyHold::with_key_up();
    // A tap released 80 ms later: up the instant it is released, no timeout to wait for.
    h.press(Key::Enter, 1.0);
    assert!(h.down(1.04).contains(&Key::Enter));
    h.release(Key::Enter);
    assert!(h.down(1.08).is_empty(), "a tap adds no delay");
    // A tap whose down and up land in the same frame is never seen as down.
    h.press(Key::Enter, 2.0);
    h.release(Key::Enter);
    assert!(h.down(2.0).is_empty());
    // A hold with a long pause before the first repeat (700 ms) is still down at 600 ms and after.
    h.press(Key::Enter, 5.0);
    assert!(h.down(5.6).contains(&Key::Enter));
    assert!(h.down(5.8).contains(&Key::Enter));
    assert_eq!(h.held_secs(Key::Enter, 5.7).map(|s| (s * 10.0).round()), Some(7.0));
    h.release(Key::Enter);
    assert!(h.down(5.9).is_empty());
    // A lost key-up does not leave the key stuck forever.
    h.press(Key::Enter, 10.0);
    assert!(!h.down(16.0).contains(&Key::Enter));
}

#[test]
fn pp_hold_first_key_up_switches_a_timing_tracker_to_exact_mode() {
    let mut h = KeyHold::default();
    h.press(Key::Enter, 0.0);
    h.release(Key::Enter);
    h.press(Key::Enter, 5.0);
    assert!(h.down(7.0).contains(&Key::Enter));
}

#[test]
fn pp_hold_new_press_after_gap_restarts_the_clock() {
    let mut h = KeyHold::default();
    h.press(Key::Enter, 0.0);
    h.press(Key::Enter, 0.2);
    h.press(Key::Enter, 3.0);
    assert_eq!(h.held_secs(Key::Enter, 3.1).map(|s| (s * 10.0).round()), Some(1.0));
}
