//! FIX-1 (D2) rules that need no server: the title logo keeps its transparency (#12); playlist names (#9).
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

/// FIX-1 #9: typing starts a fresh playlist name; leaving the box empty uses the film's name.
#[test]
fn new_playlist_name_is_typed_or_the_suggestion() {
    use jf_core::ui::playlists::chosen_name;
    assert_eq!(chosen_name("", "Big Buck Bunny"), "Big Buck Bunny");
    assert_eq!(chosen_name("  ", "Big Buck Bunny"), "Big Buck Bunny");
    assert_eq!(chosen_name("Spring", "Big Buck Bunny"), "Spring");
    assert_eq!(chosen_name(" Spring ", ""), "Spring");
    assert_eq!(chosen_name("", ""), "");
}
