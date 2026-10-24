//! FIX-1 (D2) rules that need no server: the title logo keeps its transparency (#12); playlist names (#9 / FIX-2 #2); the page's default subtitle (FIX-2 #17); dark logos (FIX-2 #14); collection covers (FIX-2 #15).
use jf_core::api::{Client, DeviceInfo, Item};

#[test]
fn logo_is_asked_for_as_png() {
    let c = Client::new("http://server.test", DeviceInfo { name: "t".into(), id: "d".into(), version: "1".into() });
    let mut it = Item { id: "abc".into(), ..Default::default() };
    assert_eq!(c.logo_url(&it, 560), None, "no logo tag → no logo (the title text is drawn)");
    it.image_tags.insert("Logo".into(), "tag1".into());
    let u = c.logo_url(&it, 560).unwrap();
    assert!(u.contains("/Items/abc/Images/Logo") && u.contains("format=png") && u.contains("tag=tag1"), "{u}");
}

/// FIX-2 #2: the box's hint is a plain label (FIX-3 A4: the "Use …" pill is gone).
#[test]
fn new_playlist_hint_is_a_label() {
    use jf_core::ui::playlists::name_hint;
    assert_eq!(name_hint(false), "Playlist name");
    assert_eq!(name_hint(true), "Collection name");
}

/// FIX-2 #17: a file whose subtitle is flagged default shows that subtitle on the page (the player turns it on).
#[test]
fn page_subtitle_follows_the_files_default_flag() {
    use jf_core::api::{MediaSource, MediaStream, StreamKind};
    use jf_core::ui::item::page_default_subtitle;
    let sub = |index, is_default, is_forced| MediaStream {
        index,
        kind: StreamKind::Subtitle,
        is_default,
        is_forced,
        ..Default::default()
    };
    let mut ms = MediaSource { media_streams: vec![sub(2, false, false), sub(3, true, false)], ..Default::default() };
    assert_eq!(page_default_subtitle(&ms), Some(3));
    ms.media_streams = vec![sub(2, false, false), sub(4, false, true)];
    assert_eq!(page_default_subtitle(&ms), Some(4), "forced counts when nothing is default");
    ms.media_streams = vec![sub(2, false, false)];
    assert_eq!(page_default_subtitle(&ms), None, "no flag → Off");
    ms.default_subtitle_stream_index = Some(2);
    assert_eq!(page_default_subtitle(&ms), Some(2), "the server's own default wins");
}

/// FIX-2 #14: dark logos are found by their visible pixels' brightness (transparent pixels don't count).
#[test]
fn dark_logo_detection() {
    use jf_core::ui::at_details::logo_is_dark;
    let png = |rgba: [u8; 4]| {
        let img = image::RgbaImage::from_pixel(8, 4, image::Rgba(rgba));
        let mut out = std::io::Cursor::new(vec![]);
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    };
    assert!(logo_is_dark(&png([10, 10, 20, 255])), "black logo");
    assert!(!logo_is_dark(&png([240, 240, 240, 255])), "white logo");
    assert!(!logo_is_dark(&png([0, 0, 0, 0])), "fully transparent: nothing to judge");
    assert!(!logo_is_dark(b"not an image"));
}

/// FIX-2 #15: a collection without its own artwork shows one of its films' posters; one with art keeps its own.
#[test]
fn collection_cover_from_a_film() {
    use jf_core::api::Item;
    use jf_core::ui::collection::with_cover;
    let coll = Item { id: "c1".into(), name: "Saga".into(), ..Default::default() };
    let mut film = Item { id: "f1".into(), ..Default::default() };
    film.image_tags.insert("Primary".into(), "t1".into());
    let shown = with_cover(&coll, Some(&film));
    assert_eq!((shown.id.as_str(), shown.name.as_str()), ("f1", "Saga"));
    assert_eq!(shown.image_tags.get("Primary").map(String::as_str), Some("t1"));
    let mut own = coll.clone();
    own.image_tags.insert("Primary".into(), "own".into());
    assert_eq!(with_cover(&own, Some(&film)).id, "c1");
    assert_eq!(with_cover(&coll, None).id, "c1");
}
