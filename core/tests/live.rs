//! Live API tests against the throwaway server ONLY (`JF_TEST_URL`, e.g. http://jf-test:8096; user tester /
//! test-pass-1; library from tools/testserver). Skipped when JF_TEST_URL is unset. Run with --test-threads=1.
//! Written by the lead; builders make them pass, never edit them.
use jf_core::api::*;

fn url() -> Option<String> {
    let u = std::env::var("JF_TEST_URL").ok();
    if u.is_none() {
        eprintln!("JF_TEST_URL unset: live test skipped");
    }
    u
}
fn dev() -> DeviceInfo {
    DeviceInfo { name: "live-test".into(), id: "live-test-device".into(), version: "0.1.0".into() }
}
fn signed(base: &str) -> Client {
    let (mut c, _) = Client::connect(base, dev()).unwrap();
    c.sign_in("tester", "test-pass-1").unwrap();
    c
}
fn view(c: &Client, name: &str) -> Item {
    c.views().unwrap().into_iter().find(|v| v.name == name).unwrap()
}
fn series(c: &Client) -> Item {
    c.library_page(&view(c, "Shows").id, 0, 10).unwrap().items.remove(0)
}
fn episode(c: &Client, s: i32, e: i32) -> Item {
    let ser = series(c);
    c.episodes(&ser.id, None)
        .unwrap()
        .into_iter()
        .find(|x| x.parent_index_number == Some(s) && x.index_number == Some(e))
        .unwrap()
}

#[test]
fn connect_and_version() {
    let Some(u) = url() else { return };
    let (c, info) = Client::connect(&u, dev()).unwrap();
    assert!(info.version.starts_with("10.10"), "{info:?}");
    assert!(!info.id.is_empty());
    assert_eq!(c.base, u.trim_end_matches('/'));
}

// R-API-2: no port given → retries :8096
#[test]
fn connect_without_port_tries_8096() {
    let Some(u) = url() else { return };
    let host = u.trim_start_matches("http://").split(':').next().unwrap().to_string();
    let (c, _) = Client::connect(&host, dev()).unwrap();
    assert_eq!(c.base, format!("http://{host}:8096"));
}

#[test]
fn sign_in_wrong_password_is_unauthorized() {
    let Some(u) = url() else { return };
    let (mut c, _) = Client::connect(&u, dev()).unwrap();
    assert_eq!(c.sign_in("tester", "nope").unwrap_err(), ApiError::Unauthorized);
    assert!(c.session.is_none());
}

#[test]
fn sign_in_verify_and_sign_out() {
    let Some(u) = url() else { return };
    let mut c = signed(&u);
    let s = c.session.clone().unwrap();
    assert_eq!(s.user_name, "tester");
    assert!(!s.token.is_empty() && !s.user_id.is_empty());
    c.verify_session().unwrap();
    let copy = c.clone();
    c.sign_out().unwrap();
    assert!(c.session.is_none());
    assert_eq!(copy.verify_session().unwrap_err(), ApiError::Unauthorized, "token must be dead after sign out");
}

#[test]
fn bogus_token_is_unauthorized() {
    let Some(u) = url() else { return };
    let mut c = signed(&u);
    c.session.as_mut().unwrap().token = "0000deadbeef".into();
    assert_eq!(c.views().unwrap_err(), ApiError::Unauthorized);
}

#[test]
fn public_users_call_works() {
    let Some(u) = url() else { return };
    let (c, _) = Client::connect(&u, dev()).unwrap();
    c.public_users().unwrap();
}

#[test]
fn views_and_library_paging() {
    let Some(u) = url() else { return };
    let c = signed(&u);
    let views = c.views().unwrap();
    // "Playlists" appears while any playlist exists (playlist tests create and delete them).
    let names: Vec<_> = views.iter().map(|v| v.name.as_str()).filter(|n| *n != "Playlists").collect();
    assert_eq!(names, ["Archive", "Collections", "Mixed", "Movies", "Shows"]);
    let movies = view(&c, "Movies");
    assert_eq!(movies.collection_type.as_deref(), Some("movies"));
    let p = c.library_page(&movies.id, 0, 100).unwrap();
    assert_eq!(p.total_record_count, 8); // 6 generated + Harbour Crossing and Three Rooms (RR6 extras)
    let names: Vec<_> = p.items.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Amber Tide",
            "Blue Harbour",
            "Copper Sky",
            "Distant Signal",
            "Echo Valley",
            "First Light",
            "Harbour Crossing (2016)",
            "Three Rooms"
        ]
    );
    let p2 = c.library_page(&movies.id, 2, 2).unwrap();
    assert_eq!(p2.items.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), ["Copper Sky", "Distant Signal"]);
    assert_eq!(p2.total_record_count, 8);
    let arch = c.library_page(&view(&c, "Archive").id, 400, 100).unwrap();
    assert_eq!((arch.total_record_count, arch.items.len()), (500, 100));
    assert_eq!(arch.items[0].name, "Archive Film 401");
    let shows = c.library_page(&view(&c, "Shows").id, 0, 100).unwrap();
    assert_eq!(shows.items.len(), 1);
    assert_eq!(shows.items[0].kind, ItemKind::Series);
}

#[test]
fn latest_and_item_detail() {
    let Some(u) = url() else { return };
    let c = signed(&u);
    let latest = c.latest(&view(&c, "Movies").id, 20).unwrap();
    assert!(!latest.is_empty() && latest.len() <= 8);
    assert!(latest.iter().all(|i| i.kind == ItemKind::Movie));
    let amber = latest.iter().find(|i| i.name == "Amber Tide").expect("Amber Tide in latest");
    let m = c.item(&amber.id).unwrap();
    assert_eq!(m.overview.as_deref(), Some("A short generated test film."));
    assert!(!m.media_sources.is_empty());
    assert!(m.run_time_ticks.unwrap_or(0) > 30 * 10_000_000);
    assert!(m.user_data.is_some());
    assert_eq!(c.item("f00df00df00df00df00df00df00df00d").unwrap_err(), ApiError::NotFound);
}

#[test]
fn seasons_episodes_next() {
    let Some(u) = url() else { return };
    let c = signed(&u);
    let ser = series(&c);
    assert_eq!(ser.name, "Night Shift Stories");
    let seasons = c.seasons(&ser.id).unwrap();
    assert_eq!(seasons.len(), 2);
    let s2 = c.episodes(&ser.id, Some(&seasons[1].id)).unwrap();
    assert_eq!(s2.iter().map(|e| e.index_number.unwrap()).collect::<Vec<_>>(), [1, 2, 3]);
    assert!(s2.iter().all(|e| e.parent_index_number == Some(2) && e.kind == ItemKind::Episode));
    assert_eq!(c.episodes(&ser.id, None).unwrap().len(), 6);
    let e13 = episode(&c, 1, 3);
    let next = c.next_episode(&ser.id, &e13.id).unwrap().unwrap();
    assert_eq!((next.parent_index_number, next.index_number), (Some(2), Some(1)));
    assert!(c.next_episode(&ser.id, &episode(&c, 2, 3).id).unwrap().is_none());
}

#[test]
fn search_finds_films_and_shows() {
    let Some(u) = url() else { return };
    let c = signed(&u);
    let r = c.search("Copper").unwrap();
    assert!(r.iter().any(|i| i.name == "Copper Sky" && i.kind == ItemKind::Movie), "{r:?}");
    let r = c.search("Night Shift").unwrap();
    assert!(r.iter().any(|i| i.kind == ItemKind::Series));
    assert!(c.search("zzqqxx-nothing").unwrap().is_empty());
}

#[test]
fn playback_info_streams() {
    let Some(u) = url() else { return };
    let c = signed(&u);
    let ep = episode(&c, 1, 1);
    let pi = c.playback_info(&ep.id).unwrap();
    assert!(pi.play_session_id.is_some());
    let ms = &pi.media_sources[0];
    assert!(ms.supports_direct_play);
    let audio: Vec<_> = ms.media_streams.iter().filter(|s| s.kind == StreamKind::Audio).collect();
    assert_eq!(audio.iter().map(|a| a.language.as_deref().unwrap_or("")).collect::<Vec<_>>(), ["eng", "fre"]);
    let ext = ms.media_streams.iter().find(|s| s.kind == StreamKind::Subtitle && s.is_external).unwrap();
    let sub_url = c.subtitle_url(&ep.id, &ms.id, ext).unwrap();
    let srt = String::from_utf8(c.get_bytes(&sub_url).unwrap()).unwrap();
    assert!(srt.contains("Deutsch line one"), "{srt}");
    let head = c.get_bytes(&c.stream_url(&ep.id, &ms.id)).unwrap();
    assert_eq!(&head[..4], &[0x1a, 0x45, 0xdf, 0xa3], "stream URL must return the mkv");
}

// R-API-22/23 + R-PLAY-3: progress lands on the server; cleaned up afterwards.
#[test]
fn progress_reporting_and_played() {
    let Some(u) = url() else { return };
    let c = signed(&u);
    let ep = episode(&c, 2, 2);
    let pi = c.playback_info(&ep.id).unwrap();
    let mut r = ProgressReport {
        item_id: ep.id.clone(),
        media_source_id: pi.media_sources[0].id.clone(),
        play_session_id: pi.play_session_id.clone(),
        audio_stream_index: Some(3),
        subtitle_stream_index: None,
        ..Default::default()
    };
    c.report_start(&r).unwrap();
    r.position_ticks = secs_to_ticks(6.0);
    c.report_progress(&r).unwrap();
    r.is_paused = true;
    r.position_ticks = secs_to_ticks(8.0);
    c.report_progress(&r).unwrap();
    c.report_stopped(&r).unwrap();
    let ud = c.item(&ep.id).unwrap().user_data.unwrap();
    assert!((ticks_to_secs(ud.playback_position_ticks) - 8.0).abs() < 1.0, "{ud:?}");
    assert!(c.resume(20).unwrap().iter().any(|i| i.id == ep.id), "stopped mid-way → Continue watching");
    c.set_played(&ep.id, true).unwrap();
    assert!(c.item(&ep.id).unwrap().user_data.unwrap().played);
    c.set_played(&ep.id, false).unwrap();
    let ud = c.item(&ep.id).unwrap().user_data.unwrap();
    assert!(!ud.played);
    // tidy: zero the position so later runs start clean
    r.position_ticks = 0;
    c.report_stopped(&r).unwrap();
}

#[test]
fn next_up_call_works() {
    let Some(u) = url() else { return };
    let c = signed(&u);
    c.next_up(20).unwrap();
}

// R-API-11: full Quick Connect round trip; the "other device" approval is done with a raw call as tester.
#[test]
fn quick_connect_round_trip() {
    let Some(u) = url() else { return };
    let (mut c, _) = Client::connect(&u, dev()).unwrap();
    assert!(c.quick_connect_enabled().unwrap());
    let st = c.quick_connect_initiate().unwrap();
    assert_eq!(st.code.len(), 6);
    assert!(!st.secret.is_empty());
    assert!(!c.quick_connect_poll(&st.secret).unwrap());
    let other = signed(&u);
    let r = ureq::post(&format!("{}/QuickConnect/Authorize?code={}", other.base, st.code))
        .header("Authorization", &other.auth_header())
        .send_empty();
    assert!(r.is_ok(), "approve failed: {r:?}");
    assert!(c.quick_connect_poll(&st.secret).unwrap());
    let s = c.quick_connect_finish(&st.secret).unwrap();
    assert_eq!(s.user_name, "tester");
    c.views().unwrap();
}

#[test]
fn images_download() {
    let Some(u) = url() else { return };
    let c = signed(&u);
    let m = &c.library_page(&view(&c, "Movies").id, 0, 1).unwrap().items[0];
    let url = c.image_url(m, ImageKind::Primary, image_width_bucket(180.0, 1.0)).unwrap();
    let b = c.get_bytes(&url).unwrap();
    assert_eq!(&b[..2], &[0xff, 0xd8], "jpeg");
}

// F5b: person pages (R-F5-1, P-PPL-1/2)
#[test]
fn person_and_their_items() {
    let Some(u) = url() else { return };
    let c = signed(&u);
    let film = c.search("Ember Rising").unwrap().into_iter().find(|i| i.kind == ItemKind::Movie).unwrap();
    let full = c.item(&film.id).unwrap();
    let ada = full.people.iter().find(|p| p.name == "Ada Quill").expect("cast carried on the item");
    assert_eq!(ada.kind, "Actor");
    let person = c.person(&ada.id).unwrap();
    assert_eq!(person.name, "Ada Quill");
    let items = c.person_items(&ada.id).unwrap();
    let mut names: Vec<_> = items.iter().map(|i| i.name.as_str()).collect();
    names.sort();
    assert_eq!(names, ["Star Drift - Beacon", "Star Drift - Ember Rising", "Star Drift - Shadow Accord"]);
}
