//! G8 (P-ITEM-11): the version chosen on the item page reaches the play request.
use jf_core::api::*;
use jf_core::playreq::{build, pick_source};
use jf_core::ui::player::PlayOptions;

fn client() -> Client {
    let dev = DeviceInfo { name: "t".into(), id: "t".into(), version: "0".into() };
    Client::new("http://jf-test:8096", dev)
}

fn info() -> PlaybackInfo {
    let src = |id: &str| MediaSource { id: id.into(), ..Default::default() };
    PlaybackInfo { media_sources: vec![src("a"), src("b")], ..Default::default() }
}

#[test]
fn chosen_source_is_used() {
    let item = Item { id: "i1".into(), name: "Film".into(), ..Default::default() };
    let opts = PlayOptions { media_source_id: Some("b".into()), ..Default::default() };
    let r = build(&client(), &item, &info(), &opts).unwrap();
    assert!(r.url.contains("b"), "{}", r.url);
    assert_eq!(pick_source(&info(), Some("b")).unwrap().id, "b");
}

#[test]
fn unknown_or_missing_source_falls_back_to_first() {
    assert_eq!(pick_source(&info(), None).unwrap().id, "a");
    assert_eq!(pick_source(&info(), Some("zzz")).unwrap().id, "a");
}
