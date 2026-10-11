//! RR5: pure rules — item link chips (P-ITEM-3), the server's Play pick (P-TV-2), unaired episodes (P-TV-7),
//! the shuffle order (P-ITEM-21).
use jf_core::api::*;
use jf_core::ui::browse::shuffled;
use jf_core::ui::item::{chips, ChipTarget};
use jf_core::ui::show_page::{airs_label, is_unaired, server_pick};

fn person(kind: &str, name: &str) -> PersonRef {
    PersonRef { id: format!("p-{name}"), name: name.into(), kind: kind.into(), ..Default::default() }
}

#[test]
fn chips_cover_genres_studios_directors_and_writers() {
    let item = Item {
        genres: vec!["Drama".into(), "Sci-Fi".into()],
        studios: vec![NameId { name: "Pike Films".into(), id: "s1".into() }],
        people: vec![
            person("Actor", "Ann"),
            person("Director", "Rowan"),
            person("Writer", "Wes"),
            person("Writer", "Wynn"),
        ],
        ..Default::default()
    };
    let c = chips(&item);
    let got: Vec<(&str, &str)> = c.iter().map(|c| (c.section, c.label.as_str())).collect();
    assert_eq!(
        got,
        vec![
            ("Genres", "Drama"),
            ("Genres", "Sci-Fi"),
            ("Studios", "Pike Films"),
            ("Director", "Rowan"),
            ("Writers", "Wes"),
            ("Writers", "Wynn")
        ]
    );
    assert_eq!(c[0].target, ChipTarget::Genre("Drama".into()));
    assert_eq!(c[2].target, ChipTarget::Studio { id: "s1".into(), name: "Pike Films".into() });
    assert_eq!(c[3].target, ChipTarget::Person { id: "p-Rowan".into(), name: "Rowan".into() });
    assert!(chips(&Item::default()).is_empty());
}

fn episode(season: i32, ep: i32) -> Item {
    Item {
        id: format!("e{season}{ep}"),
        kind: ItemKind::Episode,
        parent_index_number: Some(season),
        index_number: Some(ep),
        run_time_ticks: Some(40 * 60 * 10_000_000),
        ..Default::default()
    }
}

#[test]
fn server_pick_labels_play_and_resume() {
    let p = server_pick(&episode(3, 1)).unwrap();
    assert_eq!((p.label.as_str(), p.resume, p.episode_id.as_str()), ("Play S3 E1", false, "e31"));
    let mut half = episode(2, 4);
    half.user_data = Some(UserData { playback_position_ticks: 30 * 60 * 10_000_000, ..Default::default() });
    let p = server_pick(&half).unwrap();
    assert_eq!(p.label, "Resume S2 E4 · 10 min left");
    assert!(p.resume);
    assert!(server_pick(&Item::default()).is_none());
}

#[test]
fn unaired_means_premiere_after_today() {
    let mut e = episode(1, 1);
    e.premiere_date = Some("2026-10-14T00:00:00.0000000Z".into());
    assert!(is_unaired(&e, (2026, 10, 11)));
    assert!(!is_unaired(&e, (2026, 10, 14)));
    assert!(!is_unaired(&e, (2027, 1, 1)));
    assert_eq!(airs_label(&e).as_deref(), Some("Airs 14 Oct"));
    assert!(!is_unaired(&episode(1, 2), (2026, 10, 11)));
    assert_eq!(airs_label(&episode(1, 2)), None);
}

#[test]
fn shuffle_keeps_every_item_and_changes_with_the_seed() {
    let items: Vec<Item> = (0..20).map(|i| episode(1, i)).collect();
    let a = shuffled(items.clone(), 7);
    let b = shuffled(items.clone(), 9_999);
    let ids = |v: &[Item]| {
        let mut x: Vec<String> = v.iter().map(|i| i.id.clone()).collect();
        x.sort();
        x
    };
    assert_eq!(ids(&a), ids(&items));
    assert_ne!(a, items);
    assert_ne!(a, b);
    assert_eq!(a, shuffled(items, 7));
    assert!(shuffled(vec![], 1).is_empty());
}
