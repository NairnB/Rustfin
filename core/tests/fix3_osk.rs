//! FIX-3 (C3 osk-typing): the TV keyboard — a capital start for names, a visible Shift, short reach to Shift / 123.
use jf_core::platform::Key;
use jf_core::theme::Theme;
use jf_core::ui::keyboard::{key_filled, label, layout, on_look, Cap, Effect, KeyboardState, TvField};

fn field(name: bool, password: bool, hint: &str, text: &str) -> TvField {
    TvField { text: text.into(), password, hint: hint.into(), name }
}

fn name() -> KeyboardState {
    KeyboardState::for_field(&field(true, false, "Playlist name", ""))
}

fn focus(k: &mut KeyboardState, cap: Cap) {
    for (r, row) in k.layout().iter().enumerate() {
        for (c, cell) in row.iter().enumerate() {
            if cell.cap == cap {
                (k.row, k.col) = (r, c);
                return;
            }
        }
    }
    panic!("no key {cap:?}");
}

#[test]
fn name_field_starts_capitalised_then_lower() {
    let mut k = name();
    assert_eq!(k.focused_cap(), Cap::Char('A'));
    assert_eq!(k.press(Key::Enter), Effect::Type('A'));
    assert_eq!(k.focused_cap(), Cap::Char('a'));
    assert_eq!(k.press(Key::Enter), Effect::Type('a'));
    // a name that already has text does not start with a capital again…
    let k = KeyboardState::for_field(&field(true, false, "Playlist name", "Fam"));
    assert_eq!(k.focused_cap(), Cap::Char('a'));
    // …unless it ends in a space
    let k = KeyboardState::for_field(&field(true, false, "Playlist name", "Family "));
    assert_eq!(k.focused_cap(), Cap::Char('A'));
}

#[test]
fn space_restarts_title_case() {
    let mut k = name();
    k.press(Key::Enter); // A
    focus(&mut k, Cap::Space);
    assert_eq!(k.press(Key::Enter), Effect::Type(' '));
    focus(&mut k, Cap::Char('B'));
    assert_eq!(k.press(Key::Enter), Effect::Type('B'));
    assert!(!k.shift);
    // clear empties the field: first letter again
    focus(&mut k, Cap::Clear);
    k.press(Key::Enter);
    assert!(k.shift);
}

#[test]
fn password_never_auto_capitalises() {
    for f in [
        field(false, true, "Password", ""),
        field(false, false, "User name", ""),
        field(false, false, "Server address, e.g. 10.0.0.20:8096", ""),
        field(false, false, "Search", ""),
    ] {
        let mut k = KeyboardState::for_field(&f);
        assert_eq!(k.focused_cap(), Cap::Char('a'), "{}", f.hint);
        assert_eq!(k.press(Key::Enter), Effect::Type('a'));
        focus(&mut k, Cap::Space);
        k.press(Key::Enter);
        assert!(!k.shift, "{}", f.hint);
        assert_eq!(layout(false, k.shift)[0][1].cap, Cap::Char('a'));
    }
}

#[test]
fn shift_on_is_visible_and_keys_show_capitals() {
    let mut k = KeyboardState::for_field(&field(false, false, "Search", ""));
    assert!(!key_filled(&k, Cap::Shift));
    assert_eq!(label(Cap::Shift, false, false), "shift");
    focus(&mut k, Cap::Shift);
    k.press(Key::Enter);
    assert!(k.shift);
    assert!(key_filled(&k, Cap::Shift), "Shift on is drawn filled");
    assert!(!key_filled(&k, Cap::Space));
    assert_ne!(label(Cap::Shift, false, true), label(Cap::Shift, false, false));
    let letters: Vec<char> =
        k.layout().iter().flatten().filter_map(|c| if let Cap::Char(c) = c.cap { Some(c) } else { None }).collect();
    assert!(letters.contains(&'A') && letters.contains(&'Z') && !letters.contains(&'a'));
}

/// Fewest arrow presses from a fresh keyboard to `cap`.
fn reach(start: &KeyboardState, cap: Cap) -> usize {
    let mut frontier = vec![start.clone()];
    for n in 0..8 {
        if frontier.iter().any(|k| k.focused_cap() == cap) {
            return n;
        }
        frontier = frontier
            .iter()
            .flat_map(|k| {
                [Key::Left, Key::Right, Key::Up, Key::Down].map(|key| {
                    let mut k = k.clone();
                    k.press(key);
                    k
                })
            })
            .collect();
    }
    usize::MAX
}

#[test]
fn shift_and_layer_within_two_presses_of_start() {
    for f in
        [field(true, false, "Playlist name", ""), field(false, true, "Password", ""), field(false, false, "Search", "")]
    {
        let k = KeyboardState::for_field(&f);
        assert!(reach(&k, Cap::Shift) <= 2, "shift: {}", reach(&k, Cap::Shift));
        assert!(reach(&k, Cap::Layer) <= 2, "123: {}", reach(&k, Cap::Layer));
        let mut s = k.clone();
        s.symbols = true;
        assert!(reach(&s, Cap::Layer) <= 4);
    }
}

#[test]
fn up_and_down_leave_as_before() {
    let mut k = KeyboardState::new();
    assert_eq!(k.press(Key::Up), Effect::Leave(Key::Up));
    let mut k = KeyboardState::new();
    assert_eq!(k.press(Key::Down), Effect::None);
    assert_eq!(k.row, 1);
    let last = k.layout().len() - 1;
    k.row = last;
    k.col = 0;
    assert_eq!(k.press(Key::Down), Effect::Leave(Key::Down));
    assert_eq!(k.press(Key::Up), Effect::None);
    assert_eq!(k.row, last - 1);
}

#[test]
fn shift_on_style_differs_from_focus_style() {
    let t = Theme::placeholder();
    let (fill, outline) = on_look(&t);
    assert_ne!(fill, t.focus_fill, "Shift-on must not be the white focus plate");
    assert!(outline > 0.0);
}

#[test]
fn delete_of_first_letter_brings_shift_back() {
    let mut k = name();
    k.press(Key::Enter); // A
    assert!(!k.shift);
    k.retitle(""); // deleted it
    assert!(k.shift);
    k.retitle("Fam");
    assert!(!k.shift);
    k.retitle("Family ");
    assert!(k.shift);
    let mut p = KeyboardState::for_field(&field(false, true, "Password", ""));
    p.retitle("");
    assert!(!p.shift);
}
