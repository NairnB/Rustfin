//! G7 live tests (collections and playlists). Against the throwaway server only; skipped without `JF_TEST_URL`.
//! Viewer: create a playlist, add two Archive films, move, remove, delete it. Tester (admin): add a film to
//! "Star Drift Saga" and take it out again.
use jf_core::api::*;

fn url() -> Option<String> {
    let u = std::env::var("JF_TEST_URL").ok();
    if u.is_none() {
        eprintln!("JF_TEST_URL unset: live test skipped");
    }
    u
}
fn dev() -> DeviceInfo {
    DeviceInfo { name: "live-g7".into(), id: "live-g7-device".into(), version: "0.1.0".into() }
}

/// Delete a playlist or collection this test created. The server writes its metadata a moment after creation; a
/// delete that lands first leaves the folder behind and the next library scan brings the item back, so wait.
fn delete_settled(c: &Client, id: &str) {
    std::thread::sleep(std::time::Duration::from_secs(3));
    c.delete_item(id).unwrap();
}

fn signed_in(base: &str, user: &str, pass: &str) -> Client {
    let (mut c, _) = Client::connect(base, dev()).unwrap();
    c.sign_in(user, pass).unwrap();
    c
}

fn archive_films(c: &Client, n: usize) -> Vec<Item> {
    let arch = c.views().unwrap().into_iter().find(|v| v.name == "Archive").unwrap();
    c.library_page(&arch.id, 0, n).unwrap().items
}

#[test]
fn playlist_round_trip() {
    let Some(u) = url() else { return };
    let c = signed_in(&u, "viewer", "view-pass-1");
    let films = archive_films(&c, 3);
    assert!(films.len() >= 3);
    let (a, b, d) = (films[0].clone(), films[1].clone(), films[2].clone());

    let id = c.create_playlist("G7 live test", std::slice::from_ref(&a.id)).unwrap();
    assert!(c.playlists().unwrap().iter().any(|p| p.id == id && p.name == "G7 live test"));

    let result = (|| -> Result<()> {
        c.add_to_playlist(&id, &[b.id.clone(), d.id.clone()])?;
        let items = c.playlist_items(&id)?;
        let ids: Vec<&str> = items.iter().map(|e| e.item.id.as_str()).collect();
        assert_eq!(ids, vec![a.id.as_str(), b.id.as_str(), d.id.as_str()]);
        assert!(items.iter().all(|e| !e.entry_id.is_empty()), "every entry has a PlaylistItemId");

        // move the third entry to the front (the server alone cannot land anything at position 0)
        let entries: Vec<String> = items.iter().map(|e| e.entry_id.clone()).collect();
        c.move_playlist_item(&id, &entries, 2, 0)?;
        let moved = c.playlist_items(&id)?;
        let ids: Vec<&str> = moved.iter().map(|e| e.item.id.as_str()).collect();
        assert_eq!(ids, vec![d.id.as_str(), a.id.as_str(), b.id.as_str()]);

        // swap the first two, then the last two
        let entries: Vec<String> = moved.iter().map(|e| e.entry_id.clone()).collect();
        c.move_playlist_item(&id, &entries, 0, 1)?;
        let swapped = c.playlist_items(&id)?;
        let ids: Vec<&str> = swapped.iter().map(|e| e.item.id.as_str()).collect();
        assert_eq!(ids, vec![a.id.as_str(), d.id.as_str(), b.id.as_str()]);
        let entries: Vec<String> = swapped.iter().map(|e| e.entry_id.clone()).collect();
        c.move_playlist_item(&id, &entries, 2, 1)?;
        let again = c.playlist_items(&id)?;
        let ids: Vec<&str> = again.iter().map(|e| e.item.id.as_str()).collect();
        assert_eq!(ids, vec![a.id.as_str(), b.id.as_str(), d.id.as_str()]);
        let moved = again;

        // remove the middle one
        c.remove_from_playlist(&id, &[moved[1].entry_id.clone()])?;
        let left = c.playlist_items(&id)?;
        let ids: Vec<&str> = left.iter().map(|e| e.item.id.as_str()).collect();
        assert_eq!(ids, vec![a.id.as_str(), d.id.as_str()]);
        Ok(())
    })();

    // always put the server back
    delete_settled(&c, &id);
    assert!(c.playlists().unwrap().iter().all(|p| p.id != id), "playlist must be gone");
    result.unwrap();
}

#[test]
fn collection_list_and_add_remove() {
    let Some(u) = url() else { return };
    let c = signed_in(&u, "tester", "test-pass-1");
    let saga = c.collections().unwrap().into_iter().find(|b| b.name == "Star Drift Saga").expect("Star Drift Saga");
    assert_eq!(saga.kind, ItemKind::BoxSet);
    let before: Vec<String> = c.collection_items(&saga.id).unwrap().into_iter().map(|i| i.id).collect();

    let film = archive_films(&c, 50).into_iter().find(|f| !before.contains(&f.id)).expect("a film outside the saga");
    c.add_to_collection(&saga.id, std::slice::from_ref(&film.id)).unwrap();
    let during = c.collection_items(&saga.id).unwrap();
    assert!(during.iter().any(|i| i.id == film.id));
    assert!(c.collections_containing(&film.id).unwrap().iter().any(|b| b.id == saga.id));

    c.remove_from_collection(&saga.id, std::slice::from_ref(&film.id)).unwrap();
    let after: Vec<String> = c.collection_items(&saga.id).unwrap().into_iter().map(|i| i.id).collect();
    assert_eq!(after.len(), before.len(), "collection must be back to its original size");
    assert!(!after.contains(&film.id));
}

#[test]
fn viewer_cannot_change_collections() {
    let Some(u) = url() else { return };
    let c = signed_in(&u, "viewer", "view-pass-1");
    let saga = c.collections().unwrap().into_iter().find(|b| b.name == "Star Drift Saga").unwrap();
    let film = archive_films(&c, 1).remove(0);
    let r = c.add_to_collection(&saga.id, std::slice::from_ref(&film.id));
    if r.is_ok() {
        // the server let a non-admin through: put it back so the test server stays as it was
        c.remove_from_collection(&saga.id, &[film.id]).unwrap();
    }
    assert_eq!(r, Err(ApiError::Forbidden));
}

/// Every (from, to) pair on a four-film playlist lands exactly as asked on the real server.
#[test]
fn playlist_every_move_on_the_server() {
    let Some(u) = url() else { return };
    let c = signed_in(&u, "viewer", "view-pass-1");
    let ids: Vec<String> = archive_films(&c, 4).into_iter().map(|f| f.id).collect();
    let id = c.create_playlist("G7 live moves", &ids).unwrap();
    let result = (|| -> Result<()> {
        for from in 0..4usize {
            for to in 0..4usize {
                let before = c.playlist_items(&id)?;
                let entries: Vec<String> = before.iter().map(|e| e.entry_id.clone()).collect();
                let mut want: Vec<String> = before.iter().map(|e| e.item.id.clone()).collect();
                let e = want.remove(from);
                want.insert(to, e);
                c.move_playlist_item(&id, &entries, from, to)?;
                let got: Vec<String> = c.playlist_items(&id)?.into_iter().map(|e| e.item.id).collect();
                assert_eq!(got, want, "from {from} to {to}");
            }
        }
        Ok(())
    })();
    delete_settled(&c, &id);
    result.unwrap();
}

/// Creating a collection (admin) puts the film in it; the collection is deleted again afterwards.
#[test]
fn collection_create_and_delete() {
    let Some(u) = url() else { return };
    let c = signed_in(&u, "tester", "test-pass-1");
    let film = archive_films(&c, 1).remove(0);
    let id = c.create_collection("G7 live collection", std::slice::from_ref(&film.id)).unwrap();
    let result = (|| -> Result<()> {
        let items = c.collection_items(&id)?;
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, film.id);
        assert!(c.collections()?.iter().any(|b| b.id == id && b.name == "G7 live collection"));
        Ok(())
    })();
    delete_settled(&c, &id);
    assert!(c.collections().unwrap().iter().all(|b| b.id != id), "collection must be gone");
    result.unwrap();
}
