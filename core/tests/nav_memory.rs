//! UXP1: groups remember their last focused index; a lone control above a row of buttons goes to the first
//! button; a row of buttons stored as a vertical list still moves with Left/Right.
use jf_core::nav::{move_focus, Focus, Group};
use jf_core::platform::Key::*;

fn f(group: usize, index: usize) -> Focus {
    Focus { group, index }
}

fn row(n: usize, x0: f32) -> Group {
    Group { len: n, cols: n, xs: (0..n).map(|i| x0 + i as f32 * 100.0).collect() }
}

#[test]
fn down_then_up_returns_to_the_same_card() {
    // tabs (3), row A (5 cards, x 0..400), row B (4 cards, x 0..300)
    let g = [row(3, 0.0), row(5, 0.0), row(4, 0.0)];
    let a = move_focus(&g, f(0, 0), Down); // into row A
    assert_eq!(a, f(1, 0));
    let a = move_focus(&g, a, Right);
    let a = move_focus(&g, a, Right);
    assert_eq!(a, f(1, 2));
    let b = move_focus(&g, a, Down);
    assert_eq!(b, f(2, 2));
    let b = move_focus(&g, b, Right); // B: 3
    let back = move_focus(&g, b, Up);
    assert_eq!(back, f(1, 2), "row A remembers card 2, not nearest x of card 3");
    let again = move_focus(&g, back, Down);
    assert_eq!(again, f(2, 3), "row B remembers card 3");
}

#[test]
fn up_into_a_tab_row_lands_on_the_last_tab_focused() {
    let g = [row(4, 0.0), row(8, 0.0)];
    let t = move_focus(&g, f(0, 2), Enter); // tab 2 chosen
    assert_eq!(t, f(0, 2));
    let c = move_focus(&g, f(0, 2), Down);
    let c = move_focus(&g, move_focus(&g, c, Right), Right);
    assert_eq!(c.group, 1);
    let up = move_focus(&g, f(1, 7), Up);
    assert_eq!(up, f(0, 2), "current tab, not the tab nearest card 7");
}

#[test]
fn lone_field_above_buttons_goes_to_first_button() {
    // wide field centred at x 300, buttons Create (x 100) / Cancel (x 400): nearest would be Cancel
    let g = [Group { len: 1, cols: 1, xs: vec![300.0] }, Group { len: 2, cols: 2, xs: vec![100.0, 400.0] }];
    assert_eq!(move_focus(&g, f(0, 0), Down), f(1, 0));
}

#[test]
fn lone_search_field_down_goes_to_first_result() {
    let g = [Group { len: 1, cols: 1, xs: vec![960.0] }, row(6, 100.0)];
    assert_eq!(move_focus(&g, f(0, 0), Down), f(1, 0));
}

#[test]
fn button_row_stored_as_a_list_moves_left_and_right() {
    // Sign out / Change server / Server admin side by side: cols 1, ascending x
    let g = [Group { len: 3, cols: 1, xs: vec![100.0, 300.0, 520.0] }];
    assert_eq!(move_focus(&g, f(0, 0), Right), f(0, 1));
    assert_eq!(move_focus(&g, f(0, 1), Right), f(0, 2));
    assert_eq!(move_focus(&g, f(0, 2), Right), f(0, 2));
    assert_eq!(move_focus(&g, f(0, 2), Left), f(0, 1));
    assert_eq!(move_focus(&g, f(0, 0), Left), f(0, 0));
}

#[test]
fn true_vertical_list_ignores_left_right() {
    let g = [Group { len: 3, cols: 1, xs: vec![500.0, 500.0, 500.0] }];
    assert_eq!(move_focus(&g, f(0, 1), Right), f(0, 1));
    assert_eq!(move_focus(&g, f(0, 1), Left), f(0, 1));
}

#[test]
fn stale_memory_is_clamped() {
    let g = [row(2, 0.0), row(5, 0.0)];
    let _ = move_focus(&g, f(1, 4), Up);
    let g2 = [row(2, 0.0), row(3, 0.0)]; // different shape: memory must not index past the end
    let d = move_focus(&g2, f(0, 0), Down);
    assert!(d.index < 3);
}
