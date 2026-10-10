//! F3 (R-F3-1/3): the year line, the card subtitle with the two switches, the story-year editor labels.
use jf_core::api::*;
use jf_core::story::year_line;
use jf_core::tags::{Era, StoryYear};
use jf_core::ui::{admin_actions, parts};

fn film(year: Option<i32>, tags: &[&str]) -> Item {
    Item {
        id: "x".into(),
        name: "Star Drift".into(),
        production_year: year,
        tags: tags.iter().map(|s| s.to_string()).collect(),
        ..Default::default()
    }
}

#[test]
fn year_line_joins_release_and_story() {
    let it = film(Some(1977), &["rf-story:0 BBY"]);
    assert_eq!(year_line(&it, true, true), "1977 · 0 BBY");
    assert_eq!(year_line(&it, true, false), "1977");
    assert_eq!(year_line(&it, false, true), "0 BBY");
    assert_eq!(year_line(&it, false, false), "");
}

#[test]
fn year_line_with_missing_parts() {
    assert_eq!(year_line(&film(Some(1999), &[]), true, true), "1999");
    assert_eq!(year_line(&film(None, &["rf-story:4 ABY"]), true, true), "4 ABY");
    assert_eq!(year_line(&film(None, &[]), true, true), "");
    assert_eq!(year_line(&film(Some(1999), &["rf-story:junk"]), true, true), "1999");
}

#[test]
fn card_subtitle_follows_the_switches() {
    let it = film(Some(1977), &["rf-story:0 BBY"]);
    assert_eq!(parts::card_labels_with(&it, true, true).1, "1977 · 0 BBY");
    assert_eq!(parts::card_labels_with(&it, false, true).1, "0 BBY");
    assert_eq!(parts::card_labels(&it).1, "1977 · 0 BBY");
    assert_eq!(parts::meta_line_with(&it, true, false), "1977");
    assert_eq!(parts::meta_line_with(&it, true, true), "1977 · 0 BBY");
}

#[test]
fn editor_labels_and_value() {
    assert_eq!(admin_actions::story_label(&film(None, &[])), "Story year… (edit)");
    assert_eq!(admin_actions::story_label(&film(None, &["rf-story:19 BBY"])), "Story year: 19 BBY (edit)");
    assert_eq!(admin_actions::story_value(&film(None, &["rf-story:19 BBY"])), (19, Era::Bby));
    assert_eq!(admin_actions::story_value(&film(None, &[])), (0, Era::Bby));
    assert_eq!(StoryYear { n: 4, era: Era::Aby }.label(), "4 ABY");
}
