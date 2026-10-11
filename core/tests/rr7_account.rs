//! RR7: parental-rating sort, banner view mode, and Remember me (P-LIB-7, P-LIB-14, P-AUTH-5). Pure, no network.
use jf_core::api::{Session, SortBy};
use jf_core::platform::Storage;
use jf_core::settings::*;

fn entry() -> ServerEntry {
    ServerEntry {
        id: "srv1".into(),
        name: "Test".into(),
        address: "http://example.test:8096".into(),
        session: Some(Session { user_id: "u1".into(), user_name: "viewer".into(), token: "tok-secret".into() }),
    }
}

#[test]
fn parental_rating_sort_uses_official_rating() {
    assert_eq!(SortBy::ParentalRating.server_value(), Some("OfficialRating"));
    assert_eq!(SortBy::ParentalRating.label(), "Parental rating");
    assert!(!SortBy::ParentalRating.is_local());
}

#[test]
fn banners_is_a_view_mode_and_round_trips() {
    assert!(ViewMode::ALL.contains(&ViewMode::Banners));
    assert_eq!(ViewMode::Banners.label(), "Banners");
    let v = LibraryView { mode: ViewMode::Banners, sort: SortBy::ParentalRating, ..Default::default() };
    let s = serde_json::to_string(&v).unwrap();
    let back: LibraryView = serde_json::from_str(&s).unwrap();
    assert_eq!(back, v);
}

#[test]
fn remember_on_stores_the_session() {
    let store = MemStorage::default();
    let mut s = Settings::default();
    s.remember_signed_in(entry(), true);
    s.save(&store).unwrap();
    let back = Settings::load(&store);
    assert_eq!(back.last().unwrap().session.as_ref().unwrap().token, "tok-secret");
}

#[test]
fn remember_off_leaves_no_session_in_storage() {
    let store = MemStorage::default();
    let mut s = Settings::default();
    s.remember_signed_in(entry(), false);
    s.save(&store).unwrap();
    let raw = String::from_utf8(store.read(SETTINGS_KEY).unwrap()).unwrap();
    assert!(!raw.contains("tok-secret"), "token must not be written");
    let back = Settings::load(&store);
    assert_eq!(back.last().unwrap().address, "http://example.test:8096");
    assert!(back.last().unwrap().session.is_none());
}

#[test]
fn remember_off_clears_an_older_saved_session() {
    let store = MemStorage::default();
    let mut s = Settings::default();
    s.remember_signed_in(entry(), true);
    s.remember_signed_in(entry(), false);
    s.save(&store).unwrap();
    assert!(Settings::load(&store).last().unwrap().session.is_none());
}
