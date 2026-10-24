//! Poster picker unit rules (PP). Pure functions only — no server, no client. The candidate fixtures are the
//! JSON shape `/Items/{id}/RemoteImages?type=Primary&includeAllLanguages=true` answers with.
use jf_core::api::poster::*;
use jf_core::api::ApiError;
use std::collections::HashMap;

/// A small fixture list of remote image candidates, as the server would send them.
fn fixtures() -> Vec<RemoteImage> {
    let json = r#"[
      {"ProviderName":"TheMovieDb","Url":"https://tmdb/a.jpg","ThumbnailUrl":"https://tmdb/a-t.jpg",
       "Height":3000,"Width":2000,"CommunityRating":8.2,"VoteCount":1400,"Language":"en","Type":"Primary"},
      {"ProviderName":"Fanart","Url":"https://fanart/b.jpg","ThumbnailUrl":"https://fanart/b-t.jpg",
       "Height":2000,"Width":1333,"CommunityRating":9.1,"VoteCount":30,"Language":"en","Type":"Primary"},
      {"ProviderName":"TheMovieDb","Url":"https://tmdb/c.jpg","ThumbnailUrl":"https://tmdb/c-t.jpg",
       "Height":1500,"Width":1000,"CommunityRating":8.2,"VoteCount":900,"Language":"fr","Type":"Primary"},
      {"ProviderName":"TheMovieDb","Url":"https://tmdb/d.jpg","ThumbnailUrl":"https://tmdb/d-t.jpg",
       "Height":800,"Width":533,"CommunityRating":6.0,"VoteCount":200,"Language":null,"Type":"Primary"}
    ]"#;
    serde_json::from_str(json).unwrap()
}

#[test]
fn candidates_decode_from_the_server_shape() {
    let v = fixtures();
    assert_eq!(v.len(), 4);
    assert_eq!(v[0].provider_name.as_deref(), Some("TheMovieDb"));
    assert_eq!(v[0].language.as_deref(), Some("en"));
    assert_eq!(v[3].language, None);
}

#[test]
fn sort_is_community_rating_then_votes() {
    let sorted = sort_candidates(fixtures());
    let names: Vec<(&str, Option<f64>, Option<i32>)> =
        sorted.iter().map(|c| (c.provider_name.as_deref().unwrap_or(""), c.community_rating, c.vote_count)).collect();
    assert_eq!(
        names,
        vec![
            ("Fanart", Some(9.1), Some(30)),       // top rating wins over votes
            ("TheMovieDb", Some(8.2), Some(1400)), // equal rating → more votes first
            ("TheMovieDb", Some(8.2), Some(900)),
            ("TheMovieDb", Some(6.0), Some(200)),
        ]
    );
}

#[test]
fn chips_list_all_languages_each_language_no_text_provider_and_min_width() {
    let chips = chips_for(&fixtures());
    assert_eq!(chips[0], PosterFilter::AllLanguages);
    assert!(chips.contains(&PosterFilter::Language("en".into())));
    assert!(chips.contains(&PosterFilter::Language("fr".into())));
    assert!(chips.contains(&PosterFilter::NoText));
    assert!(chips.contains(&PosterFilter::Provider("TheMovieDb".into())));
    assert!(chips.contains(&PosterFilter::Provider("Fanart".into())));
    assert!(chips.contains(&PosterFilter::MinWidth(MIN_WIDTH_PX)));
    // All languages first, languages sorted, No text before providers.
    assert_eq!(chips[1], PosterFilter::Language("en".into()));
    assert_eq!(chips[2], PosterFilter::Language("fr".into()));
    assert_eq!(chips[3], PosterFilter::NoText);
}

#[test]
fn filters_match_language_no_text_provider_and_width() {
    let v = fixtures();
    let en = filter_candidates(&v, &PosterFilter::Language("en".into()));
    assert_eq!(en.len(), 2);
    let no_text = filter_candidates(&v, &PosterFilter::NoText);
    assert_eq!(no_text.len(), 1);
    assert_eq!(no_text[0].url.as_deref(), Some("https://tmdb/d.jpg"));
    let tmdb = filter_candidates(&v, &PosterFilter::Provider("TheMovieDb".into()));
    assert_eq!(tmdb.len(), 3);
    let wide = filter_candidates(&v, &PosterFilter::MinWidth(MIN_WIDTH_PX));
    assert_eq!(wide.len(), 3, "the 533 px poster is dropped");
}

#[test]
fn pref_key_is_rf_poster_dot_item_id() {
    assert_eq!(poster_pref_key("abc123"), "rf.poster.abc123");
}

#[test]
fn a_user_poster_override_resolves_from_custom_prefs() {
    let mut prefs: HashMap<String, Option<String>> = HashMap::new();
    prefs.insert("rf.poster.abc123".into(), Some("https://tmdb/b.jpg".into()));
    prefs.insert("rf.poster.zzz".into(), Some("https://tmdb/x.jpg".into()));
    prefs.insert("rf.poster.empty".into(), Some("".into()));
    assert_eq!(resolve_poster(&prefs, "abc123").as_deref(), Some("https://tmdb/b.jpg"));
    assert_eq!(resolve_poster(&prefs, "zzz").as_deref(), Some("https://tmdb/x.jpg"));
    assert_eq!(resolve_poster(&prefs, "empty"), None, "an empty value is no override");
    assert_eq!(resolve_poster(&prefs, "nope"), None);
    assert_eq!(resolve_poster(&HashMap::new(), "abc123"), None);
}

#[test]
fn admin_apply_backs_up_then_writes_then_refreshes() {
    let remote = admin_apply_plan(&PosterChoice::Remote { url: "https://tmdb/a.jpg".into() });
    assert_eq!(
        remote,
        vec![
            PosterStep::BackupCurrent,
            PosterStep::DownloadRemote { image_url: "https://tmdb/a.jpg".into() },
            PosterStep::Refresh,
        ]
    );
    let own = admin_apply_plan(&PosterChoice::OwnImage { bytes: vec![1, 2, 3] });
    assert_eq!(
        own,
        vec![PosterStep::BackupCurrent, PosterStep::UploadPrimary { bytes: vec![1, 2, 3] }, PosterStep::Refresh,]
    );
}

#[test]
fn viewers_never_call_a_server_image_write_endpoint() {
    // PP-8: a non-admin plan writes only the DisplayPreferences pref.
    assert_eq!(
        personal_apply_plan("https://tmdb/a.jpg"),
        vec![PosterStep::SetPersonal { url: "https://tmdb/a.jpg".into() }]
    );
    assert_eq!(
        personal_apply_plan("file:///data/posters/x.jpg"),
        vec![PosterStep::SetPersonal { url: "file:///data/posters/x.jpg".into() }]
    );
    assert_eq!(undo_plan(false, None), vec![PosterStep::ClearPersonal]);
    // None of those plans touch the write endpoints.
    for plan in [personal_apply_plan("https://tmdb/a.jpg"), undo_plan(false, None)] {
        for step in plan {
            assert!(
                !matches!(step, PosterStep::DownloadRemote { .. } | PosterStep::UploadPrimary { .. }),
                "viewer plan must not write an image: {step:?}"
            );
        }
    }
}

#[test]
fn undo_restores_the_previous_bytes_for_admins_and_clears_the_pref_for_viewers() {
    assert_eq!(undo_plan(true, Some(&[9, 9])), vec![PosterStep::UploadPrimary { bytes: vec![9, 9] }]);
    assert_eq!(undo_plan(false, Some(&[9, 9])), vec![PosterStep::ClearPersonal]);
    // An item that had no poster: undo deletes the image the apply added.
    assert_eq!(undo_plan(true, None), vec![PosterStep::DeleteImage]);
}

#[test]
fn poster_store_dir_is_beside_the_cache_and_file_urls_are_plain() {
    use std::path::Path;
    assert_eq!(poster_store_dir(Path::new("/data/cache")), std::path::PathBuf::from("/data/posters"));
    assert_eq!(file_url(Path::new("/data/posters/abc/1.jpg")), "file:///data/posters/abc/1.jpg");
}

#[test]
fn base64_decode_round_trips_the_encoder() {
    let bytes: Vec<u8> = (0..=255).collect();
    let encoded = jf_core::api::base64_encode(&bytes);
    assert_eq!(decode_base64(&encoded).as_deref(), Some(bytes.as_slice()));
    assert_eq!(decode_base64("not!base64"), None);
}

#[test]
fn the_token_only_goes_to_the_servers_own_origin() {
    use jf_core::api::{is_server_url, origin};
    assert_eq!(origin("http://jf-test:8096").as_deref(), Some("http://jf-test:8096"));
    assert_eq!(origin("HTTP://JF-TEST:8096/Items").as_deref(), Some("http://jf-test:8096"));
    assert_eq!(origin("https://example.com").as_deref(), Some("https://example.com:443"));
    assert_eq!(origin("http://example.com").as_deref(), Some("http://example.com:80"));
    assert_eq!(origin("file:///data/posters/x.jpg"), None);
    assert_eq!(origin("not a url"), None);
    // The server's own image URL shares the origin; a third party never does.
    let base = "http://jf-test:8096";
    assert!(is_server_url(base, "http://jf-test:8096/Items/abc/Images/Primary?maxWidth=200"));
    assert!(is_server_url(base, "http://jf-test:8096"));
    assert!(!is_server_url(base, "https://image.tmdb.org/t/p/original/a.jpg"));
    assert!(!is_server_url(base, "https://assets.fanart.tv/fanart/movies/1/movieposter/x.jpg"));
    assert!(!is_server_url(base, "http://jf-test:8096.evil.test/a.jpg"));
    assert!(!is_server_url(base, "http://jf-test:80960/a.jpg"));
    assert!(!is_server_url(base, "file:///data/posters/x.jpg"));
}

#[test]
fn picture_extension_uses_the_content_type_then_the_bytes() {
    let png = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0];
    assert_eq!(picture_extension(Some("image/png"), &png), "png");
    assert_eq!(picture_extension(Some("image/jpeg"), &png), "jpg");
    assert_eq!(picture_extension(None, &png), "png");
    assert_eq!(picture_extension(None, &[0xff, 0xd8, 0xff, 0xe0]), "jpg");
    assert_eq!(picture_extension(Some("image/webp"), &[0, 0, 0, 0]), "webp");
    assert_eq!(picture_extension(None, &[0, 1, 2, 3]), "jpg");
}

#[test]
fn tmdb_poster_urls_get_a_sized_variant_and_others_do_not() {
    let u = "https://image.tmdb.org/t/p/original/abcdef.jpg";
    assert_eq!(sized_poster_url(u, 280), "https://image.tmdb.org/t/p/w342/abcdef.jpg");
    assert_eq!(
        sized_poster_url("https://image.tmdb.org/t/p/w500/abcdef.jpg", 200),
        "https://image.tmdb.org/t/p/w342/abcdef.jpg"
    );
    // Above the largest standard width → keep `original`.
    assert_eq!(sized_poster_url(u, 2000), u);
    // Fanart and file paths are untouched.
    let fanart = "https://assets.fanart.tv/fanart/movies/1/movieposter/x.jpg";
    assert_eq!(sized_poster_url(fanart, 280), fanart);
    let file = "file:///data/posters/abc/1.png";
    assert_eq!(sized_poster_url(file, 280), file);
}

// ---- review round 2: token never leaves the server origin (live sockets) ----

mod leak {
    use jf_core::api::{Client, DeviceInfo, Session};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    /// A tiny HTTP server: records each request head, answers with `reply(path)`.
    fn serve(reply: impl Fn(&str) -> String + Send + 'static) -> (u16, Arc<Mutex<Vec<String>>>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let seen = Arc::new(Mutex::new(vec![]));
        let s2 = seen.clone();
        std::thread::spawn(move || {
            for conn in l.incoming() {
                let Ok(mut c) = conn else { break };
                let mut buf = [0u8; 4096];
                let n = c.read(&mut buf).unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = head.split_whitespace().nth(1).unwrap_or("/").to_string();
                s2.lock().unwrap().push(head);
                let _ = c.write_all(reply(&path).as_bytes());
            }
        });
        (port, seen)
    }

    fn ok() -> String {
        "HTTP/1.1 200 OK\r\nContent-Length: 3\r\nContent-Type: image/png\r\nConnection: close\r\n\r\nabc".to_string()
    }

    fn client(base: &str) -> Client {
        let dev = DeviceInfo { name: "t".into(), id: "d".into(), version: "1".into() };
        Client::with_session(
            base,
            dev,
            Session { user_id: "u".into(), user_name: "n".into(), token: "SECRET-TOKEN".into() },
        )
    }

    fn has_auth(seen: &Arc<Mutex<Vec<String>>>) -> bool {
        seen.lock().unwrap().iter().any(|h| {
            let l = h.to_ascii_lowercase();
            l.contains("authorization:") || l.contains("secret-token")
        })
    }

    #[test]
    fn get_bytes_sends_no_token_to_another_port_or_host() {
        let (server_port, server_seen) = serve(|_| ok());
        let (other_port, other_seen) = serve(|_| ok());
        let c = client(&format!("http://127.0.0.1:{server_port}"));
        // Same host, other port.
        assert_eq!(c.get_bytes(&format!("http://127.0.0.1:{other_port}/a.jpg")).unwrap(), b"abc");
        // Same machine, other host name.
        assert_eq!(c.get_bytes(&format!("http://localhost:{other_port}/b.jpg")).unwrap(), b"abc");
        assert_eq!(other_seen.lock().unwrap().len(), 2);
        assert!(!has_auth(&other_seen), "the token went to a third party");
        // The server's own origin still gets it.
        c.get_bytes(&format!("http://127.0.0.1:{server_port}/img")).unwrap();
        assert!(has_auth(&server_seen));
    }

    #[test]
    fn a_redirect_from_the_server_to_a_third_party_drops_the_token() {
        let (other_port, other_seen) = serve(|_| ok());
        let target = format!("http://localhost:{other_port}/final.jpg");
        let (server_port, _) = serve(move |_| {
            format!("HTTP/1.1 302 Found\r\nLocation: {target}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
        });
        let c = client(&format!("http://127.0.0.1:{server_port}"));
        assert_eq!(c.get_bytes(&format!("http://127.0.0.1:{server_port}/Items/1/Images/Primary")).unwrap(), b"abc");
        assert_eq!(other_seen.lock().unwrap().len(), 1, "the redirect was followed");
        assert!(!has_auth(&other_seen), "the token followed a redirect to a third party");
    }

    #[test]
    fn file_urls_never_go_on_the_wire() {
        let (server_port, server_seen) = serve(|_| ok());
        let c = client(&format!("http://127.0.0.1:{server_port}"));
        assert!(c.get_bytes("file:///etc/hostname").is_err());
        assert!(server_seen.lock().unwrap().is_empty());
    }
}

// ---- review round 2: file:// only under the poster store and the fixture dir ----

#[test]
fn file_urls_are_allowed_only_under_the_given_roots() {
    use std::path::PathBuf;
    let tmp = std::env::temp_dir().join(format!("pp-roots-{}", std::process::id()));
    let store = tmp.join("posters");
    let fix = tmp.join("fixture");
    let other = tmp.join("other");
    for d in [&store, &fix, &other] {
        std::fs::create_dir_all(d).unwrap();
        std::fs::write(d.join("a.png"), b"x").unwrap();
    }
    let roots: Vec<PathBuf> = vec![store.clone(), fix.clone()];
    let u = |p: PathBuf| format!("file://{}", p.display());
    assert!(file_url_allowed(&u(store.join("a.png")), &roots));
    assert!(file_url_allowed(&u(fix.join("a.png")), &roots));
    assert!(!file_url_allowed(&u(other.join("a.png")), &roots));
    assert!(!file_url_allowed("file:///etc/passwd", &roots));
    // `..` out of a root does not count
    assert!(!file_url_allowed(&u(store.join("../other/a.png")), &roots));
    // relative and non-file urls are refused
    assert!(!file_url_allowed("file://a.png", &roots));
    assert!(!file_url_allowed("https://x/a.png", &roots));
    std::fs::remove_dir_all(&tmp).unwrap();
}

#[test]
fn fixture_urls_resolve_relative_to_the_fixture_file() {
    let json = r#"{"Images":[{"Url":"file://posters/a.png","ThumbnailUrl":"file://posters/a.png"},
                   {"Url":"https://x/y.png"},{"Url":"file:///abs/b.png"}]}"#;
    let out = absolutize_fixture(json, std::path::Path::new("/work/fixture"));
    assert!(out.contains("\"file:///work/fixture/posters/a.png\""));
    assert!(out.contains("\"https://x/y.png\""));
    assert!(out.contains("\"file:///abs/b.png\""));
    assert!(!out.contains("file://posters"));
}

// ---- review round 2: the UI path runs the plan functions (recorded step log) ----

#[test]
fn run_apply_executes_the_plan_functions_in_order() {
    let remote = PosterChoice::Remote { url: "https://tmdb/x.jpg".into() };
    // Admin: backup (saved before anything is written), download, refresh.
    let mut log: Vec<PosterStep> = vec![];
    let mut saved = vec![];
    let prev = run_apply(
        true,
        &remote,
        |s| {
            log.push(s.clone());
            Ok(matches!(s, PosterStep::BackupCurrent).then(|| (vec![1, 2, 3], Some("image/png".to_string()))))
        },
        |b, ct| {
            saved.push((b.to_vec(), ct.map(str::to_string), 0));
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(log, admin_apply_plan(&remote));
    assert_eq!(prev, Some(vec![1, 2, 3]));
    assert_eq!(saved.len(), 1);

    // Own image, admin: the upload step.
    let own = PosterChoice::OwnImage { bytes: vec![9, 9] };
    let mut log = vec![];
    run_apply(
        true,
        &own,
        |s| {
            log.push(s.clone());
            Ok(None)
        },
        |_, _| Ok(()),
    )
    .unwrap();
    assert_eq!(log, admin_apply_plan(&own));

    // Viewer: only the personal pref, never a server image write.
    let mut log = vec![];
    run_apply(
        false,
        &remote,
        |s| {
            log.push(s.clone());
            Ok(None)
        },
        |_, _| Ok(()),
    )
    .unwrap();
    assert_eq!(log, personal_apply_plan("https://tmdb/x.jpg"));
    assert!(run_apply(false, &own, |_| Ok(None), |_, _| Ok(())).is_err(), "a viewer can't upload bytes");

    // A backup that cannot be saved stops the apply before the write step.
    let mut log = vec![];
    let r = run_apply(
        true,
        &remote,
        |s| {
            log.push(s.clone());
            Ok(matches!(s, PosterStep::BackupCurrent).then(|| (vec![1], None)))
        },
        |_, _| Err(ApiError::Io("disk full".into())),
    );
    assert!(r.is_err());
    assert_eq!(log, vec![PosterStep::BackupCurrent]);
}

#[test]
fn all_picks_come_from_one_custom_prefs_map() {
    let mut m: HashMap<String, Option<String>> = HashMap::new();
    m.insert(poster_pref_key("a1"), Some("https://x/a.jpg".into()));
    m.insert(poster_pref_key("b2"), Some("file:///p/b.png".into()));
    m.insert(poster_pref_key("c3"), Some(String::new()));
    m.insert(poster_pref_key("d4"), None);
    m.insert("other.key".into(), Some("zzz".into()));
    let got = all_poster_prefs(&m);
    assert_eq!(got.len(), 2);
    assert_eq!(got["a1"], "https://x/a.jpg");
    assert_eq!(got["b2"], "file:///p/b.png");
}

// ---- review round 3: a pick that can't be shown falls back to the server poster ----

#[test]
fn an_unusable_own_image_falls_back_to_the_server_poster() {
    use jf_core::ui::imageload::Images;
    let tmp = std::env::temp_dir().join(format!("pp-usable-{}", std::process::id()));
    let store = tmp.join("posters");
    std::fs::create_dir_all(&store).unwrap();
    std::fs::write(store.join("mine.png"), b"x").unwrap();
    std::fs::write(tmp.join("outside.png"), b"x").unwrap();
    let mut images = Images::new(None);
    images.set_file_roots(vec![store.clone()]);
    let u = |p: std::path::PathBuf| format!("file://{}", p.display());
    assert!(images.usable(&u(store.join("mine.png"))), "an own image in the poster store is shown");
    assert!(!images.usable(&u(store.join("gone.png"))), "missing on this device -> server poster");
    assert!(!images.usable(&u(tmp.join("outside.png"))), "not under an allowed folder -> server poster");
    assert!(!images.usable("file:///etc/passwd"));
    assert!(images.usable("https://tmdb/x.jpg"), "remote picks stay usable until they fail to load");
    std::fs::remove_dir_all(&tmp).unwrap();
}
