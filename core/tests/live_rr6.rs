//! RR6 live tests against the throwaway server ONLY (`JF_TEST_URL`; viewer / view-pass-1). Skipped without it.
//! Read-only: they use the extras made by `tools/testserver/make-extras.sh` (multi-part film, chapters, a person).
use jf_core::api::*;

fn url() -> Option<String> {
    let u = std::env::var("JF_TEST_URL").ok();
    if u.is_none() {
        eprintln!("JF_TEST_URL unset: live test skipped");
    }
    u
}

fn viewer(base: &str) -> Client {
    let dev = DeviceInfo { name: "live-rr6".into(), id: "live-rr6-device".into(), version: "0.1.0".into() };
    let (mut c, _) = Client::connect(base, dev).unwrap();
    c.sign_in("viewer", "view-pass-1").unwrap();
    c
}

fn film(c: &Client, lib: &str, name: &str) -> Item {
    let movies = c.views().unwrap().into_iter().find(|v| v.name == lib).unwrap();
    let page = c.library_page(&movies.id, 0, 100).unwrap();
    let it = page.items.into_iter().find(|i| i.name.starts_with(name)).unwrap_or_else(|| panic!("{name} on server"));
    c.item(&it.id).unwrap()
}

#[test]
fn a_two_part_film_lists_its_other_part() {
    let Some(u) = url() else { return };
    let c = viewer(&u);
    let f = film(&c, "Movies", "Harbour Crossing");
    assert_eq!(f.part_count, Some(2));
    let parts = c.additional_parts(&f.id).unwrap();
    assert_eq!(parts.len(), 1, "{parts:?}");
    assert_ne!(parts[0].id, f.id);
    // a one-part film has none
    let single = film(&c, "Movies", "Three Rooms");
    assert!(c.additional_parts(&single.id).unwrap().is_empty());
}

#[test]
fn chapter_images_are_served() {
    let Some(u) = url() else { return };
    let c = viewer(&u);
    let f = film(&c, "Movies", "Three Rooms");
    assert_eq!(f.chapters.len(), 3);
    for (i, ch) in f.chapters.iter().enumerate() {
        let tag = ch.image_tag.as_deref().unwrap_or_else(|| panic!("chapter {i} has an image tag"));
        let bytes = c.get_bytes(&c.chapter_image_url(&f.id, i, tag, 320)).unwrap();
        assert!(bytes.len() > 500, "chapter {i} image is {} bytes", bytes.len());
        assert_eq!(&bytes[..2], &[0xff, 0xd8], "chapter {i} is a jpeg");
    }
}

#[test]
fn a_person_has_born_and_died_dates() {
    let Some(u) = url() else { return };
    let c = viewer(&u);
    let f = film(&c, "Mixed", "Star Drift - Ember Rising");
    let p = f.people.iter().find(|p| p.name == "Rowan Pike").expect("Rowan Pike in the cast");
    let (born, died) = c.person_dates(&p.id).unwrap();
    assert!(born.as_deref().unwrap_or("").starts_with("1950-03-02"), "{born:?}");
    assert!(died.as_deref().unwrap_or("").starts_with("2020-07-01"), "{died:?}");
    let person = c.person(&p.id).unwrap();
    assert!(person.overview.as_deref().unwrap_or("").contains("harbours"));
}
