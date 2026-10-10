//! F2 rules (R-F2-1..3). Lead's tests; builders never edit.
use jf_core::api::*;
use jf_core::sections::*;
use jf_core::tags::with_section;

fn it(name: &str, kind: ItemKind, genres: &[&str], tags: &[&str]) -> Item {
    Item {
        id: name.into(),
        name: name.into(),
        kind,
        genres: genres.iter().map(|s| s.to_string()).collect(),
        tags: tags.iter().map(|s| s.to_string()).collect(),
        ..Default::default()
    }
}

#[test]
fn labels_round_trip() {
    for s in Section::ALL {
        assert_eq!(Section::from_name(s.label()), s);
    }
    assert_eq!(Section::TvShows.label(), "TV Shows");
    assert_eq!(Section::from_name("tv shows"), Section::TvShows);
    assert_eq!(Section::from_name("Cartoons"), Section::Other);
}

#[test]
fn automatic_placement_and_override() {
    assert_eq!(section_of(&it("a", ItemKind::Movie, &[], &[])), Section::Movies);
    assert_eq!(section_of(&it("b", ItemKind::Series, &[], &[])), Section::TvShows);
    assert_eq!(section_of(&it("c", ItemKind::Movie, &["Drama", "documentary"], &[])), Section::Documentaries);
    assert_eq!(
        section_of(&it("d", ItemKind::Series, &["Documentary"], &[])),
        Section::Documentaries,
        "genre beats type"
    );
    assert_eq!(section_of(&it("e", ItemKind::BoxSet, &[], &[])), Section::Other);
    assert_eq!(section_of(&it("f", ItemKind::Movie, &["Documentary"], &["rf-section:Specials"])), Section::Specials);
    assert_eq!(section_of(&it("g", ItemKind::Movie, &[], &["rf-section:Nowhere"])), Section::Other);
}

#[test]
fn grouping_order_and_empty_sections() {
    let items = vec![
        it("doc", ItemKind::Movie, &["Documentary"], &[]),
        it("m1", ItemKind::Movie, &[], &[]),
        it("show", ItemKind::Series, &[], &[]),
        it("m2", ItemKind::Movie, &[], &[]),
    ];
    let g = group(&items);
    let shape: Vec<(Section, Vec<&str>)> =
        g.iter().map(|(s, v)| (*s, v.iter().map(|i| i.name.as_str()).collect())).collect();
    assert_eq!(
        shape,
        vec![
            (Section::Movies, vec!["m1", "m2"]),
            (Section::TvShows, vec!["show"]),
            (Section::Documentaries, vec!["doc"])
        ]
    );
    assert!(group(&[]).is_empty());
}

// Live: the Mixed library sorts into the right boxes; an admin move is seen by another user's fresh fetch.
fn signed(base: &str, u: &str, p: &str) -> Client {
    let d = DeviceInfo { name: "sections-test".into(), id: format!("sections-{u}"), version: "0".into() };
    let (mut c, _) = Client::connect(base, d).unwrap();
    c.sign_in(u, p).unwrap();
    c
}
fn mixed(c: &Client) -> Vec<Item> {
    let v = c.views().unwrap().into_iter().find(|v| v.name == "Mixed").unwrap();
    c.library_page(&v.id, 0, 100).unwrap().items
}
fn names_in(g: &[(Section, Vec<Item>)], s: Section) -> Vec<String> {
    let mut n: Vec<String> =
        g.iter().find(|(x, _)| *x == s).map(|(_, v)| v.iter().map(|i| i.name.clone()).collect()).unwrap_or_default();
    n.sort();
    n
}

#[test]
fn live_mixed_library_and_move() {
    let Ok(u) = std::env::var("JF_TEST_URL") else { return };
    let admin = signed(&u, "tester", "test-pass-1");
    let g = group(&mixed(&admin));
    assert_eq!(names_in(&g, Section::Documentaries), ["Making Star Drift"]);
    assert_eq!(names_in(&g, Section::Specials), ["Star Drift Holiday Special"]);
    assert_eq!(names_in(&g, Section::TvShows), ["Drift Rebels"]);
    assert_eq!(names_in(&g, Section::Movies).len(), 6);
    let iron = mixed(&admin).into_iter().find(|i| i.name == "Star Drift - Iron Tide").unwrap();
    admin.update_tags(&iron.id, |t| with_section(t, Some("Specials"))).unwrap();
    let viewer = signed(&u, "viewer", "view-pass-1");
    let g2 = group(&mixed(&viewer));
    assert!(
        names_in(&g2, Section::Specials).contains(&"Star Drift - Iron Tide".to_string()),
        "second device sees the move"
    );
    admin.update_tags(&iron.id, |t| with_section(t, None)).unwrap();
    assert_eq!(names_in(&group(&mixed(&viewer)), Section::Movies).len(), 6, "Automatic puts it back");
}
