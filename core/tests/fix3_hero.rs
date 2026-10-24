//! FIX-3 tv-hero-focus (FRICTION-4 #2): on TV only the focused button is white. H1, H3, H4 as a pure style fn.
use jf_core::theme::Theme;
use jf_core::ui::widgets::play_pill_style;

#[test]
fn tv_play_pill_unfocused_is_not_white() {
    let t = Theme::placeholder();
    let s = play_pill_style(&t, true, false);
    assert!(!s.white, "an unfocused TV Play is a translucent dark plate");
    assert!(!s.rest_white);
    assert_eq!(s.scale, 1.0);
}

#[test]
fn tv_play_pill_focused_is_white_and_scaled() {
    let t = Theme::placeholder();
    let s = play_pill_style(&t, true, true);
    assert!(s.white);
    assert_eq!(s.scale, t.pill_focus_scale);
}

#[test]
fn desk_play_pill_unchanged() {
    let t = Theme::placeholder();
    for focused in [false, true] {
        let s = play_pill_style(&t, false, focused);
        assert!(s.white && s.rest_white, "desktop Play stays the white primary pill");
    }
}
