//! FIX-3 tv-search-focus (FRICTION-4 #1): after the TV keyboard lets go of the field, a result is focused.
use jf_core::nav::Focus;
use jf_core::theme::Theme;
use jf_core::ui::at_library::field_ring;
use jf_core::ui::search::{focus_after_keyboard, KbExit};

// Group order in Search: field, Top results, Films, Shows, Episodes, Collections, People, Suggested, mentions, genres.
const NONE: [usize; 10] = [1, 0, 0, 0, 0, 0, 0, 0, 0, 0];

fn lens(top: usize, films: usize, suggested: usize) -> [usize; 10] {
    [1, top, films, 0, 0, 0, 0, suggested, 0, 0]
}

/// Q1: Up out of the keyboard lands on index 0 of the first non-empty result group — the same card Down picks.
#[test]
fn leave_up_focuses_first_result() {
    assert_eq!(focus_after_keyboard(&lens(3, 2, 0), KbExit::Up), Focus { group: 1, index: 0 });
    assert_eq!(focus_after_keyboard(&lens(0, 2, 0), KbExit::Up), Focus { group: 2, index: 0 });
    assert_eq!(focus_after_keyboard(&lens(0, 0, 12), KbExit::Up), Focus { group: 7, index: 0 }, "Suggested");
    assert_eq!(focus_after_keyboard(&lens(3, 2, 0), KbExit::Up), focus_after_keyboard(&lens(3, 2, 0), KbExit::Down));
}

/// Q1: "done" and Back close the keyboard; they land on the same card.
#[test]
fn close_and_done_focus_first_result() {
    assert_eq!(focus_after_keyboard(&lens(3, 2, 0), KbExit::Close), Focus { group: 1, index: 0 });
    assert_eq!(focus_after_keyboard(&lens(0, 0, 12), KbExit::Close), Focus { group: 7, index: 0 });
}

/// Q2: nothing to land on — focus stays on the field.
#[test]
fn no_results_keeps_field_focus() {
    for exit in [KbExit::Up, KbExit::Down, KbExit::Close] {
        assert_eq!(focus_after_keyboard(&NONE, exit), Focus { group: 0, index: 0 });
    }
    // The genre buttons of an empty result are not results: Up and close leave them alone, Down still reaches them.
    let genres_only = [1, 0, 0, 0, 0, 0, 0, 0, 0, 4];
    assert_eq!(focus_after_keyboard(&genres_only, KbExit::Up), Focus { group: 0, index: 0 });
    assert_eq!(focus_after_keyboard(&genres_only, KbExit::Close), Focus { group: 0, index: 0 });
    assert_eq!(focus_after_keyboard(&genres_only, KbExit::Down), Focus { group: 9, index: 0 });
}

/// Q2: a focused field has a ring; an unfocused one has none.
#[test]
fn field_focus_look_differs_from_unfocused() {
    let t = Theme::placeholder();
    assert!(field_ring(&t, false).is_none());
    let ring = field_ring(&t, true).expect("focused field has a ring");
    assert!(ring.width > 0.0 && ring.color.a() > 0);
}
