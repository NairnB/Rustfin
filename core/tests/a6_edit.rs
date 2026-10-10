//! A6 pure rules: the metadata edit keeps every other field and the rf- tags; refresh/merge/identify paths.
use jf_core::api::item_edit::*;
use serde_json::json;

fn film() -> serde_json::Value {
    json!({
        "Id": "abc", "Name": "Star Drift", "ForcedSortName": "", "SortName": "star drift", "ProductionYear": 2001,
        "Overview": "Old.\nTwo lines.", "Genres": ["Sci-Fi"], "Tags": ["keep", "rf-section:Movies"],
        "OfficialRating": "PG", "CommunityRating": 7.5, "Studios": [{"Name": "S", "Id": "1"}],
        "People": [{"Name": "Ann", "Id": "p1", "Type": "Actor", "Role": "Pilot"}, {"Name": "Bo", "Id": "p2", "Type": "Director"}],
        "ProviderIds": {"Imdb": "tt1"}, "LockData": false
    })
}

#[test]
fn edit_reads_fields_and_hides_rf_tags() {
    let e = ItemEdit::from_json(&film());
    assert_eq!(e.name, "Star Drift");
    assert_eq!(e.year, Some(2001));
    assert_eq!(e.tags, vec!["keep".to_string()]);
    assert_eq!(e.rf_tags, vec!["rf-section:Movies".to_string()]);
    assert_eq!(e.people.len(), 2);
    assert_eq!(e.official_rating, "PG");
    assert_eq!(e.community_rating, Some(7.5));
}

#[test]
fn apply_keeps_rf_tags_and_untouched_fields() {
    let mut e = ItemEdit::from_json(&film());
    e.name = "Star Drift II".into();
    e.overview = "New".into();
    e.tags = vec!["a".into(), "b".into()];
    e.year = None;
    e.sort_name = "drift".into();
    let mut v = film();
    apply_edit(&mut v, &e);
    assert_eq!(v["Name"], "Star Drift II");
    assert_eq!(v["Overview"], "New");
    assert_eq!(v["ForcedSortName"], "drift");
    assert_eq!(v["Tags"], json!(["a", "b", "rf-section:Movies"]));
    assert!(v["ProductionYear"].is_null());
    assert_eq!(v["Studios"], film()["Studios"]);
    assert_eq!(v["ProviderIds"], film()["ProviderIds"]);
    assert_eq!(v["LockData"], false);
}

#[test]
fn unchanged_edit_is_a_no_op() {
    let mut v = film();
    apply_edit(&mut v, &ItemEdit::from_json(&film()));
    let mut want = film();
    want["ForcedSortName"] = json!("");
    assert_eq!(v, want);
}

#[test]
fn people_keep_ids_for_unchanged_and_drop_removed() {
    let mut e = ItemEdit::from_json(&film());
    e.people.remove(1);
    e.people.push(PersonEdit { name: "Cy".into(), kind: "Writer".into(), role: String::new() });
    let mut v = film();
    apply_edit(&mut v, &e);
    let p = v["People"].as_array().unwrap();
    assert_eq!(p.len(), 2);
    assert_eq!(p[0]["Id"], "p1");
    assert_eq!(p[0]["Role"], "Pilot");
    assert_eq!(p[1]["Name"], "Cy");
    assert_eq!(p[1]["Type"], "Writer");
}

#[test]
fn list_and_person_text() {
    assert_eq!(split_list(" Drama, Sci-Fi ,,"), vec!["Drama".to_string(), "Sci-Fi".to_string()]);
    assert_eq!(join_list(&["a".into(), "b".into()]), "a, b");
    let p = parse_person("Ann | Actor | Pilot").unwrap();
    assert_eq!((p.name.as_str(), p.kind.as_str(), p.role.as_str()), ("Ann", "Actor", "Pilot"));
    let p = parse_person("Bo").unwrap();
    assert_eq!(p.kind, "Actor");
    assert!(parse_person("  ").is_none());
    assert_eq!(
        person_line(&PersonEdit { name: "Bo".into(), kind: "Director".into(), role: String::new() }),
        "Bo | Director"
    );
}

#[test]
fn rating_text() {
    assert_eq!(parse_rating(""), Ok(None));
    assert_eq!(parse_rating("7.5"), Ok(Some(7.5)));
    assert!(parse_rating("11").is_err());
    assert!(parse_rating("x").is_err());
    assert_eq!(parse_year(""), Ok(None));
    assert_eq!(parse_year("1999"), Ok(Some(1999)));
    assert!(parse_year("99999").is_err());
}

#[test]
fn paths() {
    assert_eq!(
        refresh_path("i1", RefreshMode::Scan, false),
        "/Items/i1/Refresh?Recursive=true&ImageRefreshMode=Default&MetadataRefreshMode=Default&ReplaceAllImages=false&ReplaceAllMetadata=false"
    );
    assert!(refresh_path("i1", RefreshMode::Missing, false)
        .contains("MetadataRefreshMode=FullRefresh&ReplaceAllImages=false&ReplaceAllMetadata=false"));
    assert!(refresh_path("i1", RefreshMode::Replace, true).contains("ReplaceAllImages=true&ReplaceAllMetadata=true"));
    assert_eq!(merge_path(&["a".into(), "b".into()]), "/Videos/MergeVersions?Ids=a,b");
    assert_eq!(
        web_link("http://h:8096", "i1", Some("srv")),
        "http://h:8096/web/index.html#!/details?id=i1&serverId=srv"
    );
    assert_eq!(web_link("http://h:8096", "i1", None), "http://h:8096/web/index.html#!/details?id=i1");
}

#[test]
fn remote_matches_read_and_roundtrip() {
    let raw = json!({"Name": "Star Drift", "ProductionYear": 2001, "SearchProviderName": "TheMovieDb", "ProviderIds": {"Tmdb": "5"}, "Overview": "x"});
    let m = RemoteMatch::from_json(raw.clone());
    assert_eq!(m.label(), "Star Drift (2001) · TheMovieDb");
    assert_eq!(m.raw, raw);
    assert_eq!(identify_kind("Movie"), Some("Movie"));
    assert_eq!(identify_kind("Series"), Some("Series"));
    assert_eq!(identify_kind("Episode"), None);
}

#[test]
fn more_menu_rows() {
    use jf_core::api::ItemKind;
    use jf_core::ui::admin_actions::{more_entries, MoreAction};
    let names = |k: &ItemKind, admin, n| more_entries(k, admin, n).into_iter().map(|(l, _)| l).collect::<Vec<_>>();
    assert_eq!(names(&ItemKind::Movie, false, 2), ["Copy link"]);
    assert_eq!(
        names(&ItemKind::Movie, true, 1),
        ["Edit metadata", "Identify", "Refresh metadata", "Merge versions", "Copy link", "Delete"]
    );
    assert!(names(&ItemKind::Movie, true, 2).contains(&"Split versions"));
    assert_eq!(names(&ItemKind::Episode, true, 1), ["Edit metadata", "Refresh metadata", "Copy link", "Delete"]);
    assert_eq!(names(&ItemKind::Series, true, 0)[1], "Identify");
    assert_eq!(more_entries(&ItemKind::Movie, true, 1).last().unwrap().1, MoreAction::Delete);
}
