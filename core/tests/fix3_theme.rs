//! P-USER-2: the Dark / Black theme choice — saved with the other display settings, and Black swaps only the background tokens.

use jf_core::settings::{Settings, ThemeChoice};
use jf_core::theme::Theme;

#[test]
fn theme_choice_defaults_to_dark_and_round_trips() {
    assert_eq!(Settings::default().theme, ThemeChoice::Dark);
    let old: Settings = serde_json::from_str(r#"{"device_id":"x"}"#).unwrap();
    assert_eq!(old.theme, ThemeChoice::Dark);
    let s = Settings { theme: ThemeChoice::Black, ..Default::default() };
    let back: Settings = serde_json::from_slice(&serde_json::to_vec(&s).unwrap()).unwrap();
    assert_eq!(back.theme, ThemeChoice::Black);
    assert_eq!(back, s);
}

#[test]
fn black_changes_only_the_background_tokens() {
    let dark = Theme::for_choice(ThemeChoice::Dark);
    assert_eq!(dark, Theme::placeholder());
    let black = Theme::for_choice(ThemeChoice::Black);
    assert_eq!(black.bg, egui::Color32::BLACK);
    assert_eq!(black.sidebar_fill, egui::Color32::BLACK);
    // Everything but the background tokens is the same: put the dark ones back and the two are equal.
    let mut restored = black.clone();
    restored.bg = dark.bg;
    restored.sidebar_fill = dark.sidebar_fill;
    assert_eq!(restored, dark);
}
