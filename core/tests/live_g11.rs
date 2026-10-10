//! G11 tests: library filter params (P-LIB-10..12), recommendation titles (P-LIB-16), and live calls for the
//! genres / studios / upcoming / suggestions / filter-options / episodes views (P-LIB-16, 18..21).
//! The live ones skip without `JF_TEST_URL`; they only read.
use jf_core::api::*;

fn url() -> Option<String> {
    let u = std::env::var("JF_TEST_URL").ok();
    if u.is_none() {
        eprintln!("JF_TEST_URL unset: live test skipped");
    }
    u
}
fn signed(base: &str) -> Client {
    let dev = DeviceInfo { name: "live-test".into(), id: "live-test-g11".into(), version: "0.1.0".into() };
    let (mut c, _) = Client::connect(base, dev).unwrap();
    c.sign_in("viewer", "view-pass-1").unwrap();
    c
}
fn path_for(q: &LibraryQuery) -> String {
    let mut p = String::new();
    push_filters(&mut p, q);
    p
}

#[test]
fn default_query_adds_nothing() {
    assert_eq!(path_for(&LibraryQuery::default()), "");
}

#[test]
fn basic_filters_still_join_in_order() {
    let q = LibraryQuery {
        filters: Filters { unplayed: true, favourites: true, ..Default::default() },
        ..Default::default()
    };
    assert_eq!(path_for(&q), "&Filters=IsUnplayed,IsFavorite");
}

#[test]
fn extra_filters_become_server_params() {
    let q = LibraryQuery {
        extra: ExtraFilters {
            genres: vec!["Sci-Fi".into(), "Drama".into()],
            ratings: vec!["PG-13".into()],
            tags: vec!["kids".into()],
            year_from: Some(2001),
            year_to: Some(2003),
            has_subtitles: true,
            has_trailer: true,
            has_special_features: true,
            has_theme_song: true,
            has_theme_video: true,
            hd: true,
            uhd: true,
            three_d: true,
            studio: Some(("abc123".into(), "Studio One".into())),
        },
        ..Default::default()
    };
    let p = path_for(&q);
    for want in [
        "&Genres=Sci-Fi%7CDrama",
        "&OfficialRatings=PG-13",
        "&Tags=kids",
        "&Years=2001,2002,2003",
        "&StudioIds=abc123",
        "&HasSubtitles=true",
        "&HasTrailer=true",
        "&HasSpecialFeature=true",
        "&HasThemeSong=true",
        "&HasThemeVideo=true",
        "&IsHD=true",
        "&Is4K=true",
        "&Is3D=true",
    ] {
        assert!(p.contains(want), "{want} missing from {p}");
    }
}

#[test]
fn genre_names_are_percent_encoded() {
    let q = LibraryQuery {
        extra: ExtraFilters { genres: vec!["Action & Adventure".into()], ..Default::default() },
        ..Default::default()
    };
    assert_eq!(path_for(&q), "&Genres=Action%20%26%20Adventure");
}

#[test]
fn year_range_either_way_round() {
    let x = ExtraFilters { year_from: Some(2005), year_to: Some(2003), ..Default::default() };
    assert_eq!(x.years(), vec![2003, 2004, 2005]);
    assert!(ExtraFilters::default().years().is_empty());
    assert!(ExtraFilters::default().is_empty());
    assert!(!ExtraFilters { hd: true, ..Default::default() }.is_empty());
}

#[test]
fn summary_names_each_choice() {
    let x = ExtraFilters {
        genres: vec!["Drama".into()],
        year_from: Some(1990),
        year_to: Some(1999),
        uhd: true,
        studio: Some(("i".into(), "BBC".into())),
        ..Default::default()
    };
    assert_eq!(x.summary(), "BBC · Drama · 1990–1999 · 4K");
}

#[test]
fn old_saved_settings_still_load() {
    // A LibraryView saved before G11 has no `extra` or `mode`.
    let v: jf_core::settings::LibraryView =
        serde_json::from_str(r#"{"sort":"Name","descending":true,"filters":{"unplayed":true,"played":false,"resumable":false,"favourites":false}}"#)
            .unwrap();
    assert!(v.descending && v.filters.unplayed);
    assert!(v.extra.is_empty());
    assert_eq!(v.mode, jf_core::settings::ViewMode::Posters);
    let back: jf_core::settings::LibraryView = serde_json::from_str(&serde_json::to_string(&v).unwrap()).unwrap();
    assert_eq!(back, v);
}

#[test]
fn recommendation_titles() {
    let r = |t: &str, b: &str| Recommendation {
        recommendation_type: t.into(),
        baseline_item_name: Some(b.into()),
        items: vec![],
    };
    assert_eq!(r("SimilarToRecentlyPlayed", "Heat").title(), "Because you watched Heat");
    assert_eq!(r("SimilarToLikedItem", "Heat").title(), "Because you liked Heat");
    assert_eq!(r("HasDirectorFromRecentlyPlayed", "Mann").title(), "Directed by Mann");
    assert_eq!(r("HasLikedActor", "Pacino").title(), "Starring Pacino");
    assert_eq!(r("Other", "x").title(), "Suggested for you");
}

#[test]
fn library_views_load_on_the_server() {
    let Some(u) = url() else { return };
    let c = signed(&u);
    let views = c.views().unwrap();
    let mixed = views.iter().find(|v| v.name == "Mixed").expect("Mixed library");
    // P-LIB-18: the Mixed library has the genre Documentary.
    let genres = c.genres(&mixed.id, "Movie,Series").unwrap();
    assert!(genres.iter().any(|g| g.name == "Documentary"), "genres: {genres:?}");
    // P-LIB-10: the filter options list it too, and filtering by it narrows the list.
    let opts = c.filter_options(&mixed.id, "Movie,Series").unwrap();
    assert!(opts.genres.iter().any(|g| g == "Documentary"));
    assert!(!opts.years.is_empty());
    let all = c.library_query(&mixed.id, 0, 100, &LibraryQuery::default()).unwrap();
    let q = LibraryQuery {
        extra: ExtraFilters { genres: vec!["Documentary".into()], ..Default::default() },
        ..Default::default()
    };
    let docs = c.library_query(&mixed.id, 0, 100, &q).unwrap();
    assert!(docs.total_record_count > 0 && docs.total_record_count < all.total_record_count);
    assert!(docs.items.iter().all(|i| i.genres.iter().any(|g| g == "Documentary")));
    // P-LIB-10: a year range keeps only items from those years.
    let q = LibraryQuery {
        extra: ExtraFilters { year_from: Some(2002), year_to: Some(2005), ..Default::default() },
        ..Default::default()
    };
    let yrs = c.library_query(&mixed.id, 0, 100, &q).unwrap();
    assert!(yrs.total_record_count < all.total_record_count);
    assert!(yrs.items.iter().all(|i| matches!(i.production_year, Some(2002..=2005))));
    // P-LIB-11/12: flag filters are accepted by the server (counts can only shrink).
    let q = LibraryQuery {
        extra: ExtraFilters { has_subtitles: true, hd: true, ..Default::default() },
        ..Default::default()
    };
    let flagged = c.library_query(&mixed.id, 0, 100, &q).unwrap();
    assert!(flagged.total_record_count <= all.total_record_count);
    // P-LIB-20 / 19 / 16: these calls answer (the throwaway server has no studios, upcoming or history).
    c.studios(&mixed.id, "Movie,Series").unwrap();
    c.upcoming(&mixed.id).unwrap();
    c.recommendations(&mixed.id).unwrap();
}

#[test]
fn episodes_tab_lists_episodes() {
    let Some(u) = url() else { return };
    let c = signed(&u);
    let views = c.views().unwrap();
    let shows = views.iter().find(|v| v.name == "Shows").expect("Shows library");
    let eps = c.library_query_types(&shows.id, 0, 100, &LibraryQuery::default(), "Episode").unwrap();
    assert!(eps.total_record_count > 0);
    assert!(eps.items.iter().all(|i| i.kind == ItemKind::Episode));
}
