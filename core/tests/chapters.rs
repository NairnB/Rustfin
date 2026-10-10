//! G4 chapter-list model (P-PLAY-10, P-ITEM-17). Pure unit tests, no server needed.
use jf_core::api::Chapter;
use jf_core::ui::player::chapter_rows;

fn ch(ticks: i64, name: Option<&str>, tag: Option<&str>) -> Chapter {
    Chapter { start_position_ticks: ticks, name: name.map(str::to_string), image_tag: tag.map(str::to_string) }
}

#[test]
fn chapter_rows_names_and_times() {
    // 0 s, 60 s, 120 s — already in order.
    let chapters =
        vec![ch(0, Some("Opening"), None), ch(600_000_000, Some("Middle"), None), ch(1_200_000_000, None, None)];
    let rows = chapter_rows(&chapters);
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0], (0.0, "0:00 · Opening".to_string()));
    assert_eq!(rows[1], (60.0, "1:00 · Middle".to_string()));
    // Missing name → "Chapter N" (1-based).
    assert_eq!(rows[2], (120.0, "2:00 · Chapter 3".to_string()));
}

#[test]
fn chapter_rows_sort_by_start() {
    let chapters =
        vec![ch(1_200_000_000, Some("Last"), None), ch(0, Some("First"), None), ch(600_000_000, Some("Mid"), None)];
    let rows = chapter_rows(&chapters);
    let secs: Vec<f64> = rows.iter().map(|(s, _)| *s).collect();
    assert_eq!(secs, vec![0.0, 60.0, 120.0]);
    assert_eq!(rows[0].1, "0:00 · First".to_string());
}

#[test]
fn chapter_rows_empty() {
    assert!(chapter_rows(&[]).is_empty());
    // A chapter with an empty name gets a fallback label, not an empty string.
    let rows = chapter_rows(&[ch(5_000_000, Some("  "), Some("t"))]);
    assert_eq!(rows, vec![(0.5, "0:00 · Chapter 1".to_string())]);
}
