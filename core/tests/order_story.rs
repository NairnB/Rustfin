//! F4 + F3 pure rules (R-F4-1, R-F3-1/2). Lead's tests; builders never edit.
use jf_core::api::*;
use jf_core::order::*;
use jf_core::story::*;

fn it(name: &str, tags: &[&str]) -> Item {
    Item {
        id: name.into(),
        name: name.into(),
        tags: tags.iter().map(|s| s.to_string()).collect(),
        ..Default::default()
    }
}
fn names(v: &[Item]) -> Vec<&str> {
    v.iter().map(|i| i.name.as_str()).collect()
}
const P: &str = "abcd1234";

#[test]
fn my_order_sort() {
    let mut v = vec![
        it("zeta", &[]),
        it("b", &["rf-order:abcd1234:20"]),
        it("Alpha", &[]),
        it("a", &["rf-order:abcd1234:10", "rf-order:ffff0000:99"]),
        it("c", &["rf-order:ffff0000:1"]),
    ];
    sort_my_order(&mut v, P);
    assert_eq!(names(&v), ["a", "b", "Alpha", "c", "zeta"]);
}

#[test]
fn move_with_a_gap_is_one_write() {
    let shown = vec![
        it("a", &["rf-order:abcd1234:10"]),
        it("b", &["rf-order:abcd1234:20"]),
        it("c", &["rf-order:abcd1234:30"]),
    ];
    assert_eq!(move_writes(&shown, P, 2, 0), vec![("c".to_string(), 5)]);
    assert_eq!(move_writes(&shown, P, 0, 1), vec![("a".to_string(), 25)]);
    assert_eq!(move_writes(&shown, P, 0, 2), vec![("a".to_string(), 40)], "to the end: prev + 20 → middle");
    assert!(move_writes(&shown, P, 1, 1).is_empty());
}

#[test]
fn move_without_a_gap_renumbers() {
    let shown = vec![
        it("a", &["rf-order:abcd1234:10"]),
        it("b", &["rf-order:abcd1234:11"]),
        it("c", &["rf-order:abcd1234:12"]),
    ];
    // c between a and b: no gap between 10 and 11 → renumber a, c (k = 1): a=10 (unchanged), c=20
    assert_eq!(move_writes(&shown, P, 2, 1), vec![("c".to_string(), 20)]);
}

#[test]
fn moving_into_unordered_items_renumbers_the_prefix() {
    let shown = vec![it("a", &["rf-order:abcd1234:10"]), it("x", &[]), it("y", &[]), it("z", &[])];
    // z to index 2 → new order a, x, z, y; x before k has no position → renumber a=10, x=20, z=30
    assert_eq!(move_writes(&shown, P, 3, 2), vec![("x".to_string(), 20), ("z".to_string(), 30)]);
    let none = vec![it("p", &[]), it("q", &[])];
    assert_eq!(move_writes(&none, P, 1, 0), vec![("q".to_string(), 10)]);
}

#[test]
fn story_sort_and_detection() {
    let mut v = vec![
        it("Iron Tide", &["rf-story:3 ABY"]),
        it("Beacon", &["rf-story:0 BBY"]),
        it("Making of", &[]),
        it("Ember", &["rf-story:32 BBY"]),
        it("Broken", &["rf-story:soon"]),
    ];
    assert!(any_story(&v));
    sort_story(&mut v);
    assert_eq!(names(&v), ["Ember", "Beacon", "Iron Tide", "Broken", "Making of"]);
    assert!(!any_story(&[it("x", &["rf-story:soon"])]));
}

#[test]
fn year_lines() {
    let mut b = it("Beacon", &["rf-story:0 BBY"]);
    b.production_year = Some(1977);
    assert_eq!(year_line(&b, true, true), "1977 · 0 BBY");
    assert_eq!(year_line(&b, true, false), "1977");
    assert_eq!(year_line(&b, false, true), "0 BBY");
    assert_eq!(year_line(&b, false, false), "");
    let mut plain = it("x", &[]);
    plain.production_year = Some(2001);
    assert_eq!(year_line(&plain, true, true), "2001");
    assert_eq!(year_line(&it("y", &[]), true, true), "");
}
