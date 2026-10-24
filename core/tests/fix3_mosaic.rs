//! FIX-3 #9 (playlist-mosaic): a playlist tile is drawn from its items' own posters.
use egui::{pos2, Rect};
use jf_core::api::{Item, ItemKind};
use jf_core::ui::playlists::{mosaic_pick, mosaic_rects, tile_art, TileArt};

fn film(id: &str, tag: &str) -> Item {
    let mut i = Item { id: id.into(), name: id.into(), kind: ItemKind::Movie, ..Default::default() };
    i.image_tags.insert("Primary".into(), tag.into());
    i
}

fn episode(id: &str, series: &str) -> Item {
    let mut i = Item { id: id.into(), kind: ItemKind::Episode, ..Default::default() };
    i.image_tags.insert("Primary".into(), format!("still-{id}"));
    i.series_id = Some(series.into());
    i.series_primary_image_tag = Some(format!("poster-{series}"));
    i
}

fn ids(v: &[Item]) -> Vec<&str> {
    v.iter().map(|i| i.id.as_str()).collect()
}

const TILE: Rect = Rect { min: pos2(0.0, 0.0), max: pos2(200.0, 300.0) };

#[test]
fn mosaic_dedupes_items() {
    let a = film("a", "ta");
    let got = mosaic_pick(&[a.clone(), a.clone(), film("b", "tb"), a]);
    assert_eq!(ids(&got), ["a", "b"], "same item id once");
    let got = mosaic_pick(&[episode("e1", "s1"), episode("e2", "s1"), episode("e3", "s2")]);
    assert_eq!(ids(&got), ["s1", "s2"], "episodes of one series once, drawn from the series poster");
    assert_eq!(got[0].image_tags.get("Primary").map(String::as_str), Some("poster-s1"));
    assert!(mosaic_pick(&[Item { id: "x".into(), ..Default::default() }]).is_empty(), "no poster, not drawn");
}

#[test]
fn one_item_is_its_poster() {
    let r = mosaic_rects(1, TILE);
    assert_eq!(r, vec![TILE]);
}

#[test]
fn two_items_split() {
    let r = mosaic_rects(2, TILE);
    assert_eq!(r.len(), 2);
    assert_eq!(r[0], Rect::from_min_max(pos2(0.0, 0.0), pos2(100.0, 300.0)));
    assert_eq!(r[1], Rect::from_min_max(pos2(100.0, 0.0), pos2(200.0, 300.0)));
}

#[test]
fn three_items_layout() {
    let r = mosaic_rects(3, TILE);
    assert_eq!(r.len(), 3);
    assert_eq!(r[0], Rect::from_min_max(pos2(0.0, 0.0), pos2(100.0, 300.0)), "one tall half");
    assert_eq!(r[1], Rect::from_min_max(pos2(100.0, 0.0), pos2(200.0, 150.0)));
    assert_eq!(r[2], Rect::from_min_max(pos2(100.0, 150.0), pos2(200.0, 300.0)));
}

#[test]
fn four_plus_items_first_four_distinct() {
    let r = mosaic_rects(4, TILE);
    assert_eq!(r.len(), 4);
    assert_eq!(r[3], Rect::from_min_max(pos2(100.0, 150.0), pos2(200.0, 300.0)));
    assert_eq!(mosaic_rects(7, TILE), r, "more than four still draws four");
    let many: Vec<Item> = (0..9).map(|n| film(&format!("f{n}"), "t")).collect();
    assert_eq!(ids(&mosaic_pick(&many)), ["f0", "f1", "f2", "f3"]);
    let mut dup = vec![film("f0", "t"), film("f0", "t")];
    dup.extend((1..6).map(|n| film(&format!("f{n}"), "t")));
    assert_eq!(ids(&mosaic_pick(&dup)), ["f0", "f1", "f2", "f3"], "duplicates don't use up a slot");
}

#[test]
fn empty_playlist_uses_server_image_or_placeholder() {
    let mut own = Item { id: "p".into(), ..Default::default() };
    assert!(matches!(tile_art(&[], &own), TileArt::Placeholder));
    own.image_tags.insert("Primary".into(), "srv".into());
    assert!(matches!(tile_art(&[], &own), TileArt::Server));
    assert!(matches!(tile_art(&[film("a", "t")], &own), TileArt::Posters(p) if p.len() == 1));
    assert!(mosaic_rects(0, TILE).is_empty());
}
