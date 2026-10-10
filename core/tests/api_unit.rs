//! Pure API rules (no server). SPEC §4. Written by the lead; builders make them pass, never edit them.
use jf_core::api::*;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}
fn dev() -> DeviceInfo {
    DeviceInfo { name: "Test PC".into(), id: "dev-1".into(), version: "0.1.0".into() }
}
fn signed() -> Client {
    Client::with_session(
        "http://h:8096",
        dev(),
        Session { user_id: "u1".into(), user_name: "tester".into(), token: "tok".into() },
    )
}

// R-API-2
#[test]
fn normalise_adds_scheme_and_trims() {
    assert_eq!(normalise_address(" 10.0.0.5:8096 ").unwrap(), "http://10.0.0.5:8096");
    assert_eq!(normalise_address("http://h:8096/").unwrap(), "http://h:8096");
    assert_eq!(normalise_address("https://media.example.org/jellyfin/").unwrap(), "https://media.example.org/jellyfin");
    assert_eq!(normalise_address("HTTP://H:8096").unwrap(), "http://H:8096");
    assert_eq!(normalise_address("jf-test").unwrap(), "http://jf-test");
}
#[test]
fn normalise_rejects_junk() {
    assert!(matches!(normalise_address(""), Err(ApiError::BadAddress(_))));
    assert!(matches!(normalise_address("   "), Err(ApiError::BadAddress(_))));
    assert!(matches!(normalise_address("my server"), Err(ApiError::BadAddress(_))));
    assert!(matches!(normalise_address("ftp://h"), Err(ApiError::BadAddress(_))));
}

// R-API-3
#[test]
fn version_gate() {
    assert!(version_supported("10.10.7"));
    assert!(version_supported("10.8.0"));
    assert!(version_supported("10.11.2"));
    assert!(version_supported("11.0.0"));
    assert!(!version_supported("10.7.7"));
    assert!(!version_supported("10.2.0"));
    assert!(version_supported("unstable-build"));
}

// R-API-31
#[test]
fn image_width_buckets() {
    assert_eq!(image_width_bucket(180.0, 1.0), 200);
    assert_eq!(image_width_bucket(200.0, 1.0), 200);
    assert_eq!(image_width_bucket(201.0, 1.0), 240);
    assert_eq!(image_width_bucket(180.0, 2.0), 360);
    assert_eq!(image_width_bucket(1.0, 1.0), 40);
    assert_eq!(image_width_bucket(0.0, 1.0), 40);
}

// R-API-32
#[test]
fn ticks() {
    assert_eq!(ticks_to_secs(10_000_000), 1.0);
    assert_eq!(secs_to_ticks(2.5), 25_000_000);
    assert_eq!(secs_to_ticks(ticks_to_secs(123_456_789)), 123_456_789);
}

// R-API-1
#[test]
fn auth_header_before_and_after_sign_in() {
    let c = Client::new("http://h:8096", dev());
    let h = c.auth_header();
    assert!(h.starts_with("MediaBrowser "), "{h}");
    assert!(h.contains("Client=\"Rustfin\""), "{h}");
    assert!(h.contains("Device=\"Test PC\""), "{h}");
    assert!(h.contains("DeviceId=\"dev-1\""), "{h}");
    assert!(h.contains("Version=\"0.1.0\""), "{h}");
    assert!(!h.contains("Token="), "{h}");
    assert!(signed().auth_header().contains("Token=\"tok\""));
}
#[test]
fn auth_header_escapes_quotes_in_device_name() {
    let mut d = dev();
    d.name = "Living \"Room\" PC".into();
    let h = Client::new("http://h:8096", d).auth_header();
    assert!(!h.contains("\"Room\""), "quotes must not break the header: {h}");
}

// R-API-30
#[test]
fn stream_and_subtitle_urls() {
    let c = signed();
    assert_eq!(c.stream_url("abc", "ms1"), "http://h:8096/Videos/abc/stream?static=true&mediaSourceId=ms1&api_key=tok");
    let ext = MediaStream {
        index: 0,
        kind: StreamKind::Subtitle,
        codec: Some("subrip".into()),
        is_external: true,
        ..Default::default()
    };
    assert_eq!(
        c.subtitle_url("abc", "ms1", &ext).unwrap(),
        "http://h:8096/Videos/abc/ms1/Subtitles/0/0/Stream.srt?api_key=tok"
    );
    let ass = MediaStream { index: 3, codec: Some("ass".into()), ..ext.clone() };
    assert!(c.subtitle_url("abc", "ms1", &ass).unwrap().contains("/Subtitles/3/0/Stream.ass?"));
    let vtt = MediaStream { index: 4, codec: Some("webvtt".into()), ..ext.clone() };
    assert!(c.subtitle_url("abc", "ms1", &vtt).unwrap().contains("/Stream.vtt?"));
    let embedded = MediaStream { is_external: false, ..ext.clone() };
    assert_eq!(c.subtitle_url("abc", "ms1", &embedded), None);
    let unknown = MediaStream { codec: Some("pgssub".into()), ..ext };
    assert_eq!(c.subtitle_url("abc", "ms1", &unknown), None);
}

// R-API-31
#[test]
fn image_urls_and_fallbacks() {
    let c = signed();
    let mut tags = HashMap::new();
    tags.insert("Primary".to_string(), "t1".to_string());
    let movie = Item {
        id: "m1".into(),
        kind: ItemKind::Movie,
        image_tags: tags,
        backdrop_image_tags: vec!["b1".into()],
        ..Default::default()
    };
    assert_eq!(
        c.image_url(&movie, ImageKind::Primary, 200).unwrap(),
        "http://h:8096/Items/m1/Images/Primary?maxWidth=200&quality=90&tag=t1"
    );
    assert_eq!(
        c.image_url(&movie, ImageKind::Backdrop, 1920).unwrap(),
        "http://h:8096/Items/m1/Images/Backdrop?maxWidth=1920&quality=90&tag=b1"
    );
    let ep = Item {
        id: "e1".into(),
        kind: ItemKind::Episode,
        series_id: Some("s1".into()),
        series_primary_image_tag: Some("st".into()),
        parent_backdrop_item_id: Some("s1".into()),
        parent_backdrop_image_tags: vec!["pb".into()],
        ..Default::default()
    };
    assert_eq!(
        c.image_url(&ep, ImageKind::Primary, 400).unwrap(),
        "http://h:8096/Items/s1/Images/Primary?maxWidth=400&quality=90&tag=st"
    );
    assert_eq!(
        c.image_url(&ep, ImageKind::Backdrop, 1920).unwrap(),
        "http://h:8096/Items/s1/Images/Backdrop?maxWidth=1920&quality=90&tag=pb"
    );
    let bare = Item { id: "x".into(), ..Default::default() };
    assert_eq!(c.image_url(&bare, ImageKind::Primary, 200), None);
    assert_eq!(c.image_url(&bare, ImageKind::Backdrop, 200), None);
    assert!(!c.image_url(&movie, ImageKind::Primary, 200).unwrap().contains("api_key"));
}

// Fixtures decode (R-API-3: unknown fields ignored)
#[test]
fn fixtures_decode() {
    let info: PublicInfo = serde_json::from_str(&fixture("info_public.json")).unwrap();
    assert!(info.version.starts_with("10.10"));
    let views: Page = serde_json::from_str(&fixture("views.json")).unwrap();
    let names: Vec<_> = views.items.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(names, ["Archive", "Movies", "Shows"]);
    assert!(views.items.iter().all(|i| i.kind == ItemKind::CollectionFolder));
    assert_eq!(views.items[2].collection_type.as_deref(), Some("tvshows"));
    let movies: Page = serde_json::from_str(&fixture("movies_page.json")).unwrap();
    assert_eq!(movies.total_record_count, 6);
    assert_eq!(movies.items[0].name, "Amber Tide");
    assert_eq!(movies.items[0].production_year, Some(2019));
    assert!(movies.items[0].image_tags.contains_key("Primary"));
    let seasons: Page = serde_json::from_str(&fixture("seasons.json")).unwrap();
    assert_eq!(seasons.items.len(), 2);
    assert!(seasons.items.iter().all(|s| s.kind == ItemKind::Season));
    let eps: Page = serde_json::from_str(&fixture("episodes.json")).unwrap();
    assert_eq!(eps.items.len(), 6);
    assert_eq!((eps.items[3].parent_index_number, eps.items[3].index_number), (Some(2), Some(1)));
    let ep: Item = serde_json::from_str(&fixture("episode_item.json")).unwrap();
    assert_eq!(ep.kind, ItemKind::Episode);
    assert_eq!(ep.series_name.as_deref(), Some("Night Shift Stories"));
    assert!(ep.user_data.is_some());
    let pi: PlaybackInfo = serde_json::from_str(&fixture("playbackinfo.json")).unwrap();
    let ms = &pi.media_sources[0];
    assert_eq!(ms.container.as_deref(), Some("mkv"));
    assert!(pi.play_session_id.is_some());
    let audio = ms.media_streams.iter().filter(|s| s.kind == StreamKind::Audio).count();
    let ext_subs = ms.media_streams.iter().filter(|s| s.kind == StreamKind::Subtitle && s.is_external).count();
    let emb_subs = ms.media_streams.iter().filter(|s| s.kind == StreamKind::Subtitle && !s.is_external).count();
    assert_eq!((audio, ext_subs, emb_subs), (2, 1, 1));
}

#[test]
fn unknown_item_type_is_other() {
    let i: Item = serde_json::from_str(r#"{"Id":"1","Name":"n","Type":"MusicAlbum","Brand":"new"}"#).unwrap();
    assert_eq!(i.kind, ItemKind::Other);
}

#[test]
fn progress_report_is_pascal_case() {
    let r =
        ProgressReport { item_id: "i".into(), media_source_id: "m".into(), position_ticks: 5, ..Default::default() };
    let v = serde_json::to_value(&r).unwrap();
    for k in [
        "ItemId",
        "MediaSourceId",
        "PlaySessionId",
        "PositionTicks",
        "IsPaused",
        "AudioStreamIndex",
        "SubtitleStreamIndex",
    ] {
        assert!(v.get(k).is_some(), "missing {k}");
    }
}

/// Tiny HTTP stub: answers each request by path via `route`, records request lines.
fn stub(route: fn(&str) -> (u16, &'static str)) -> (String, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let seen = std::sync::Arc::new(std::sync::Mutex::new(vec![]));
    let s2 = seen.clone();
    std::thread::spawn(move || {
        for conn in l.incoming() {
            let Ok(mut conn) = conn else { continue };
            let mut r = BufReader::new(conn.try_clone().unwrap());
            let mut line = String::new();
            if r.read_line(&mut line).is_err() {
                continue;
            }
            let mut len = 0usize;
            loop {
                let mut h = String::new();
                if r.read_line(&mut h).is_err() || h == "\r\n" || h.is_empty() {
                    break;
                }
                if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap_or(0);
                }
            }
            let mut body = vec![0u8; len];
            let _ = std::io::Read::read_exact(&mut r, &mut body);
            let path = line.split_whitespace().nth(1).unwrap_or("").to_string();
            s2.lock().unwrap().push(format!("{} {}", line.split_whitespace().next().unwrap_or(""), path));
            let (code, json) = route(&path);
            let _ = write!(conn, "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{json}", json.len());
        }
    });
    (base, seen)
}

// R-API-4: new route first, legacy on 404, and the choice is remembered.
#[test]
fn route_fallback_to_legacy_and_remembered() {
    let (base, seen) = stub(|p| {
        if p.starts_with("/UserViews") {
            (404, "")
        } else if p.starts_with("/Users/u1/Views") {
            (
                200,
                r#"{"Items":[{"Id":"v1","Name":"Films","Type":"CollectionFolder","CollectionType":"movies"}],"TotalRecordCount":1}"#,
            )
        } else {
            (500, "")
        }
    });
    let c = Client::with_session(
        &base,
        dev(),
        Session { user_id: "u1".into(), user_name: "t".into(), token: "tok".into() },
    );
    assert_eq!(c.views().unwrap()[0].name, "Films");
    assert_eq!(c.views().unwrap().len(), 1);
    let s = seen.lock().unwrap().clone();
    assert_eq!(
        s.iter().filter(|l| l.contains("/UserViews")).count(),
        1,
        "new route tried once, then remembered: {s:?}"
    );
    assert_eq!(s.iter().filter(|l| l.contains("/Users/u1/Views")).count(), 2, "{s:?}");
}

// R-API-5
#[test]
fn error_mapping() {
    let (base, _) = stub(|p| match p {
        p if p.starts_with("/UserViews") => (401, ""),
        p if p.starts_with("/Items/x") => (404, ""),
        p if p.starts_with("/Shows/NextUp") => (500, "boom"),
        _ => (200, "not json"),
    });
    let c = Client::with_session(
        &base,
        dev(),
        Session { user_id: "u1".into(), user_name: "t".into(), token: "tok".into() },
    );
    assert_eq!(c.views().unwrap_err(), ApiError::Unauthorized);
    assert!(matches!(c.next_up(5).unwrap_err(), ApiError::Server(500, _)));
    assert!(matches!(c.search("a").unwrap_err(), ApiError::Decode(_)));
    let dead = Client::new("http://127.0.0.1:9", dev());
    assert!(matches!(dead.public_info().unwrap_err(), ApiError::Network(_)));
}

// R-API-3 via connect
#[test]
fn connect_rejects_old_server() {
    let (base, _) = stub(|_| (200, r#"{"ServerName":"old","Version":"10.7.7","Id":"x"}"#));
    assert!(matches!(Client::connect(&base, dev()), Err(ApiError::TooOld(v)) if v == "10.7.7"));
}
