//! A6 live tests against the throwaway server ONLY (`JF_TEST_URL`; admin tester / test-pass-1, non-admin viewer /
//! view-pass-1). Skipped without it. Edits one Archive film's overview and puts the whole item back (even on a
//! failed assert). Nothing is merged, split or deleted: Delete is only tried on an id that does not exist.
use jf_core::api::item_edit::{identify_kind, ItemEdit, RefreshMode};
use jf_core::api::*;

fn url() -> Option<String> {
    let u = std::env::var("JF_TEST_URL").ok();
    if u.is_none() {
        eprintln!("JF_TEST_URL unset: live test skipped");
    }
    u
}

fn signed(base: &str, user: &str, pass: &str) -> Client {
    let dev = DeviceInfo { name: "live-a6".into(), id: "live-a6-device".into(), version: "0.1.0".into() };
    let (mut c, _) = Client::connect(base, dev).unwrap();
    c.sign_in(user, pass).unwrap();
    c
}

fn archive_film(c: &Client) -> Item {
    let arch = c.views().unwrap().into_iter().find(|v| v.name == "Archive").unwrap();
    c.library_page(&arch.id, 0, 1).unwrap().items.remove(0)
}

/// Puts the item's original editor fields back when dropped.
struct Restore {
    client: Client,
    id: String,
    edit: ItemEdit,
}

impl Drop for Restore {
    fn drop(&mut self) {
        let _ = self.client.save_item_edit(&self.id, &self.edit);
    }
}

#[test]
fn edit_overview_and_put_it_back() {
    let Some(u) = url() else { return };
    let admin = signed(&u, "tester", "test-pass-1");
    let film = archive_film(&admin);
    let before = admin.item_json(&film.id).unwrap();
    let original = ItemEdit::from_json(&before);
    let _restore = Restore { client: admin.clone(), id: film.id.clone(), edit: original.clone() };

    let mut e = original.clone();
    e.overview = format!("{} [A6 live test]", original.overview);
    e.tags.push("a6-live-tag".into());
    e.genres.push("A6 Genre".into());
    admin.save_item_edit(&film.id, &e).unwrap();

    let after = admin.item_json(&film.id).unwrap();
    let now = ItemEdit::from_json(&after);
    assert!(now.overview.ends_with("[A6 live test]"));
    assert!(now.tags.contains(&"a6-live-tag".to_string()));
    assert!(now.genres.contains(&"A6 Genre".to_string()));
    // every field the editor does not show is unchanged
    for key in ["Id", "Path", "ProviderIds", "Studios", "Taglines", "RunTimeTicks", "MediaSources"] {
        assert_eq!(before.get(key), after.get(key), "{key} changed");
    }
    assert_eq!(now.rf_tags, original.rf_tags);

    admin.save_item_edit(&film.id, &original).unwrap();
    let back = ItemEdit::from_json(&admin.item_json(&film.id).unwrap());
    assert_eq!(back.overview, original.overview);
    assert_eq!(back.tags, original.tags);
    assert_eq!(back.genres, original.genres);
}

#[test]
fn sort_title_and_people_round_trip() {
    let Some(u) = url() else { return };
    let admin = signed(&u, "tester", "test-pass-1");
    let film = archive_film(&admin);
    let original = ItemEdit::from_json(&admin.item_json(&film.id).unwrap());
    let _restore = Restore { client: admin.clone(), id: film.id.clone(), edit: original.clone() };

    let mut e = original.clone();
    e.sort_name = "zz a6 sort".into();
    e.people.push(jf_core::api::item_edit::PersonEdit {
        name: "A6 Test Person".into(),
        kind: "Actor".into(),
        role: "Tester".into(),
    });
    admin.save_item_edit(&film.id, &e).unwrap();
    let now = ItemEdit::from_json(&admin.item_json(&film.id).unwrap());
    assert_eq!(now.sort_name, "zz a6 sort");
    assert!(now.people.iter().any(|p| p.name == "A6 Test Person" && p.role == "Tester"));
}

#[test]
fn refresh_and_identify_work_or_say_no_matches() {
    let Some(u) = url() else { return };
    let admin = signed(&u, "tester", "test-pass-1");
    let film = archive_film(&admin);
    admin.refresh_item(&film.id, RefreshMode::Scan, false).unwrap();
    // The throwaway server has no internet: an empty list or a server error are both fine, a panic is not.
    let kind = identify_kind("Movie").unwrap();
    match admin.remote_search(kind, &film.id, &film.name, film.production_year) {
        Ok(list) => eprintln!("identify: {} matches", list.len()),
        Err(e) => eprintln!("identify: {e}"),
    }
}

#[test]
fn viewer_cannot_edit_refresh_or_delete() {
    let Some(u) = url() else { return };
    let admin = signed(&u, "tester", "test-pass-1");
    let film = archive_film(&admin);
    let viewer = signed(&u, "viewer", "view-pass-1");
    let e = ItemEdit::from_json(&admin.item_json(&film.id).unwrap());
    assert!(matches!(viewer.save_item_edit(&film.id, &e), Err(ApiError::Forbidden)));
    assert!(viewer.refresh_item(&film.id, RefreshMode::Scan, false).is_err());
    assert!(viewer.split_versions(&film.id).is_err());
    // still there, still unchanged
    let again = admin.item_json(&film.id).unwrap();
    assert_eq!(ItemEdit::from_json(&again).name, e.name);
}

#[test]
fn delete_of_a_missing_item_is_refused() {
    let Some(u) = url() else { return };
    let admin = signed(&u, "tester", "test-pass-1");
    assert!(admin.delete_item("00000000000000000000000000000000").is_err());
}

#[test]
fn merge_candidates_exclude_the_item_itself() {
    let Some(u) = url() else { return };
    let admin = signed(&u, "tester", "test-pass-1");
    let film = archive_film(&admin);
    let c = admin.merge_candidates(&film.id, &film.name).unwrap();
    assert!(c.iter().all(|i| i.id != film.id));
}
