//! FIX-3 add-to-flow (A1–A4): the "Add to…" menu has no dead end, opens lit on its first row, and the new-playlist
//! form has no "Use “<title>”" pill.
use jf_core::ui::item::{add_menu_focus, add_root_labels, root_focus_after_load};

const NEW: &str = "New playlist…";
const ADD: &str = "Add to playlist…";

/// A1: playlists known and none → "New playlist…" (plus the admin rows), no "Add to playlist…".
#[test]
fn add_menu_without_playlists_offers_new_playlist_only() {
    assert_eq!(add_root_labels(Some(0), false), [NEW]);
    let admin = add_root_labels(Some(0), true);
    assert_eq!(admin, [NEW, "Add to collection…", "New collection…"]);
    assert!(!admin.contains(&ADD));
    // with playlists the row is back
    assert_eq!(add_root_labels(Some(2), false), [ADD, NEW]);
}

/// A2: still loading → both entries, as before.
#[test]
fn add_menu_while_loading_unchanged() {
    assert_eq!(add_root_labels(None, false), [ADD, NEW]);
    assert_eq!(add_root_labels(None, true), [ADD, NEW, "Add to collection…", "New collection…"]);
}

/// A3: opening the menu puts focus on its first list row, as the Audio / Subtitles pickers do.
#[test]
fn add_menu_opens_on_first_row() {
    let f = add_menu_focus();
    assert_eq!(f.group, jf_core::ui::item::LIST_GROUP);
    assert_eq!(f.index, 0);
}

/// A4: no "Use “<title>”" pill; the hint stays "Playlist name"; focus starts on the name field.
#[test]
fn new_playlist_form_has_no_use_title_pill() {
    use jf_core::ui::playlists::name_hint;
    assert_eq!(name_hint(false), "Playlist name");
    let src = include_str!("../src/ui/playlists.rs");
    assert!(!src.contains("use_label"), "the pill helper is gone");
    assert!(!src.contains("N_USE"), "no pill focus group");
    assert!(!src.contains("Use \\u{201c}"), "no Use “…” label");
}

/// Review F2: playlists load empty while the root is open → "Add to playlist…" vanishes, focus follows its action up one row.
#[test]
fn root_focus_follows_its_row_when_playlists_row_disappears() {
    // admin on "New collection…" (loading index 3) stays on it (now index 2), not "Remove from…"
    assert_eq!(root_focus_after_load(3, None, Some(0)), 2);
    assert_eq!(root_focus_after_load(1, None, Some(0)), 0);
    // already on the first row: stays there
    assert_eq!(root_focus_after_load(0, None, Some(0)), 0);
    // playlists exist, or the row was already hidden: unchanged
    assert_eq!(root_focus_after_load(1, None, Some(2)), 1);
    assert_eq!(root_focus_after_load(1, Some(0), Some(0)), 1);
}
