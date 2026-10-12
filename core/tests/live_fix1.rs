//! FIX-1 #13: the plot / genre / people search behind an empty title search (skips without JF_TEST_URL).
use jf_core::api::{Client, DeviceInfo, Item};

fn signed() -> Option<Client> {
    let url = std::env::var("JF_TEST_URL").ok()?;
    let device = DeviceInfo { name: "fix1-test".into(), id: "fix1-test-device".into(), version: "0".into() };
    let (mut c, _) = Client::connect(&url, device).ok()?;
    c.sign_in("tester", "test-pass-1").ok()?;
    Some(c)
}

#[test]
fn plot_words_find_films_the_title_search_misses() {
    let Some(c) = signed() else {
        eprintln!("JF_TEST_URL unset: skipped");
        return;
    };
    // The test films' plots say "A short generated test film."; no title contains "generated".
    assert!(c.search("generated").unwrap().is_empty());
    let (found, genres) = c.search_text("generated test").unwrap();
    assert!(!found.is_empty(), "plot search found nothing");
    assert!(found.len() <= 30);
    let _: &Vec<String> = &genres;
}

#[test]
fn text_matches_every_word_case_insensitively() {
    let a = Item { name: "A".into(), overview: Some("A Robot learns to paint.".into()), ..Default::default() };
    let b = Item { name: "B".into(), genres: vec!["Documentary".into()], ..Default::default() };
    let items = vec![a, b];
    let names = |v: Vec<Item>| v.into_iter().map(|i| i.name).collect::<Vec<_>>();
    assert_eq!(names(jf_core::api::text_matches(items.clone(), "robot")), ["A"]);
    assert_eq!(names(jf_core::api::text_matches(items.clone(), "documentary")), ["B"]);
    assert!(jf_core::api::text_matches(items.clone(), "robot documentary").is_empty());
    assert!(jf_core::api::text_matches(items, "  ").is_empty());
}
