//! T1 rules: Rustfin server tags (R-TAG-1..6). Lead's tests; builders never edit.
use jf_core::api::*;
use jf_core::tags::*;

fn v(s: &[&str]) -> Vec<String> {
    s.iter().map(|x| x.to_string()).collect()
}

// R-TAG-3
#[test]
fn story_year_parse_key_label() {
    let a = StoryYear::parse("0 BBY").unwrap();
    assert_eq!((a.n, a.era), (0, Era::Bby));
    assert_eq!(StoryYear::parse(" 32 bby ").unwrap(), StoryYear { n: 32, era: Era::Bby });
    assert_eq!(StoryYear::parse("4 ABY").unwrap(), StoryYear { n: 4, era: Era::Aby });
    for bad in ["", "BBY", "4", "4 XYZ", "-3 BBY", "3.5 ABY", "4ABY ABY"] {
        assert!(StoryYear::parse(bad).is_none(), "{bad}");
    }
    let mut ys: Vec<StoryYear> =
        ["4 ABY", "32 BBY", "0 BBY", "19 BBY", "3 ABY"].iter().map(|s| StoryYear::parse(s).unwrap()).collect();
    ys.sort_by_key(|y| y.key());
    assert_eq!(ys.iter().map(|y| y.label()).collect::<Vec<_>>(), ["32 BBY", "19 BBY", "0 BBY", "3 ABY", "4 ABY"]);
}

// R-TAG-1
#[test]
fn parse_reads_only_valid_rf_tags() {
    let t = RfTags::parse(&v(&[
        "Favourite",
        "rf-section:Specials",
        "rf-story:0 BBY",
        "rf-order:ab12cd34:20",
        "rf-order:ffff0000:10",
        "rf-order:broken",
        "rf-story:nonsense",
        "RF-section:Wrong case prefix",
    ]));
    assert_eq!(t.section.as_deref(), Some("Specials"));
    assert_eq!(t.story, Some(StoryYear { n: 0, era: Era::Bby }));
    assert_eq!(t.order.get("ab12cd34"), Some(&20));
    assert_eq!(t.order.get("ffff0000"), Some(&10));
    assert_eq!(t.order.len(), 2);
    assert_eq!(RfTags::parse(&[]), RfTags::default());
}

// R-TAG-2
#[test]
fn edits_touch_only_their_key() {
    let base = v(&[
        "Favourite",
        "rf-section:Other",
        "rf-story:nonsense",
        "rf-order:aaaa0000:10",
        "rf-order:bbbb0000:5",
        "Kids",
    ]);
    assert_eq!(
        with_section(&base, Some("Specials")),
        v(&[
            "Favourite",
            "rf-story:nonsense",
            "rf-order:aaaa0000:10",
            "rf-order:bbbb0000:5",
            "Kids",
            "rf-section:Specials"
        ])
    );
    assert_eq!(
        with_section(&base, None),
        v(&["Favourite", "rf-story:nonsense", "rf-order:aaaa0000:10", "rf-order:bbbb0000:5", "Kids"])
    );
    let s = with_story(&base, Some(StoryYear { n: 19, era: Era::Bby }));
    assert!(s.contains(&"rf-story:19 BBY".to_string()));
    assert!(!s.contains(&"rf-story:nonsense".to_string()), "a story edit replaces every rf-story: tag");
    assert_eq!(s.iter().filter(|t| t.starts_with("rf-section:")).count(), 1);
    let o = with_order(&base, "aaaa0000", Some(30));
    assert_eq!(o.last().unwrap(), "rf-order:aaaa0000:30");
    assert!(o.contains(&"rf-order:bbbb0000:5".to_string()));
    assert_eq!(o.iter().filter(|t| t.starts_with("rf-order:aaaa0000:")).count(), 1);
    assert!(!with_order(&base, "aaaa0000", None).iter().any(|t| t.starts_with("rf-order:aaaa0000:")));
    assert_eq!(with_section(&v(&["rf-section:A", "rf-section:B"]), Some("C")), v(&["rf-section:C"]));
}

// R-TAG-4
#[test]
fn place_ids() {
    assert_eq!(place_id("AB12CD34EF56aa00bb11cc22dd33ee44"), "ab12cd34");
    assert_eq!(place_id("ab12cd34-ef56-aa00-bb11-cc22dd33ee44"), "ab12cd34");
}

// ---- live (throwaway server only) ----
fn url() -> Option<String> {
    std::env::var("JF_TEST_URL").ok()
}
fn dev() -> DeviceInfo {
    DeviceInfo { name: "tags-test".into(), id: "tags-test-device".into(), version: "0.1.0".into() }
}
fn signed(base: &str, user: &str, pass: &str) -> Client {
    let (mut c, _) = Client::connect(base, dev()).unwrap();
    c.sign_in(user, pass).unwrap();
    c
}
fn find(c: &Client, name: &str) -> Item {
    c.search(name).unwrap().into_iter().find(|i| i.name == name).unwrap_or_else(|| panic!("{name} not found"))
}

// R-TAG-5/6
#[test]
fn admin_flag() {
    let Some(u) = url() else { return };
    assert!(signed(&u, "tester", "test-pass-1").is_admin().unwrap());
    assert!(!signed(&u, "viewer", "view-pass-1").is_admin().unwrap());
}

#[test]
fn admin_writes_only_rf_tags_and_nothing_else_changes() {
    let Some(u) = url() else { return };
    let c = signed(&u, "tester", "test-pass-1");
    let special = find(&c, "Star Drift Holiday Special");
    assert!(
        special.tags.contains(&"rf-section:Specials".to_string()),
        "list fetch must carry Tags: {:?}",
        special.tags
    );
    let before = c.item(&special.id).unwrap();
    let new = c.update_tags(&special.id, |t| with_story(t, Some(StoryYear { n: 1, era: Era::Aby }))).unwrap();
    assert!(new.contains(&"rf-story:1 ABY".to_string()) && new.contains(&"rf-section:Specials".to_string()));
    let after = c.item(&special.id).unwrap();
    assert_eq!(RfTags::parse(&after.tags).story, Some(StoryYear { n: 1, era: Era::Aby }));
    assert_eq!(
        (&after.name, &after.overview, &after.production_year, &after.genres),
        (&before.name, &before.overview, &before.production_year, &before.genres)
    );
    // tidy: back to just the section tag
    let back = c.update_tags(&special.id, |t| with_story(t, None)).unwrap();
    assert_eq!(back, vec!["rf-section:Specials".to_string()]);
    let doc = find(&c, "Making Star Drift");
    assert_eq!(doc.genres, vec!["Documentary".to_string()], "list fetch must carry Genres");
}

#[test]
fn viewer_cannot_write() {
    let Some(u) = url() else { return };
    let v = signed(&u, "viewer", "view-pass-1");
    let doc = find(&v, "Making Star Drift");
    assert_eq!(v.update_tags(&doc.id, |t| with_section(t, Some("Other"))).unwrap_err(), ApiError::Forbidden);
    assert!(v.item(&doc.id).unwrap().tags.is_empty());
}
