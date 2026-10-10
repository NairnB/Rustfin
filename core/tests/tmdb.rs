//! F5a rules (R-F5-1/2) against a local stub serving recorded, made-up TMDB answers. No live TMDB calls.
//! Lead's tests; builders never edit.
use jf_core::api::{Item, ItemKind};
use jf_core::tmdb::*;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

fn fixture(n: &str) -> String {
    std::fs::read_to_string(format!("{}/tests/fixtures/tmdb/{n}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

/// Stub TMDB: answers by path; key "good-key" only, else 401. Records request lines.
fn stub() -> (String, Arc<Mutex<Vec<String>>>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/3", l.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(vec![]));
    let s2 = seen.clone();
    let (search, credits) = (fixture("search_person.json"), fixture("combined_credits.json"));
    std::thread::spawn(move || {
        for conn in l.incoming() {
            let Ok(mut conn) = conn else { continue };
            let mut r = BufReader::new(conn.try_clone().unwrap());
            let mut line = String::new();
            let _ = r.read_line(&mut line);
            loop {
                let mut h = String::new();
                if r.read_line(&mut h).is_err() || h == "\r\n" || h.is_empty() {
                    break;
                }
            }
            let path = line.split_whitespace().nth(1).unwrap_or("").to_string();
            s2.lock().unwrap().push(path.clone());
            let (code, body) = if !path.contains("api_key=good-key") {
                (401, r#"{"status_code":7,"status_message":"Invalid API key"}"#.to_string())
            } else if path.starts_with("/3/search/person") {
                (200, search.clone())
            } else if path.starts_with("/3/person/9100001/combined_credits") {
                (200, credits.clone())
            } else {
                (404, "{}".to_string())
            };
            let _ = write!(conn, "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        }
    });
    (base, seen)
}
fn client(base: &str, key: &str, cache: Option<std::path::PathBuf>) -> Tmdb {
    let mut t = Tmdb::new(key, cache);
    t.base = base.to_string();
    t
}

#[test]
fn find_person_and_credits() {
    let (base, _) = stub();
    let t = client(&base, "good-key", None);
    assert_eq!(t.find_person("Ada Quill").unwrap(), Some(9100001));
    let c = t.credits(9100001).unwrap();
    assert_eq!(c.iter().filter(|x| x.dept == Dept::Acting).count(), 6);
    assert_eq!(c.iter().filter(|x| x.dept == Dept::Crew).count(), 2);
    let hl = c.iter().find(|x| x.title == "Harbour Lights").unwrap();
    assert_eq!((hl.kind, hl.year, hl.role.as_str()), (MediaKind::Tv, Some(2016), "Ines"));
    let un = c.iter().find(|x| x.title == "Untitled Project").unwrap();
    assert_eq!(un.year, None);
}

#[test]
fn key_errors() {
    let (base, seen) = stub();
    assert_eq!(client(&base, "  ", None).find_person("x").unwrap_err(), TmdbError::NoKey);
    assert!(seen.lock().unwrap().is_empty(), "no request without a key");
    assert_eq!(client(&base, "wrong", None).find_person("x").unwrap_err(), TmdbError::BadKey);
}

#[test]
fn sorted_newest_first_and_merged() {
    let (base, _) = stub();
    let c = client(&base, "good-key", None).credits(9100001).unwrap();
    let (acting, crew) = split_sorted(&c);
    let t: Vec<&str> = acting.iter().map(|x| x.title.as_str()).collect();
    assert_eq!(
        t,
        [
            "Harbour Lights",
            "Quiet Orchard",
            "Star Drift: Shadow Accord",
            "Star Drift - Ember Rising",
            "Star Drift - Beacon",
            "Untitled Project"
        ]
    );
    assert_eq!(crew.len(), 1);
    assert_eq!(crew[0].role, "Director, Writer");
}

#[test]
fn cached_on_disk_without_the_key_in_file_names() {
    let (base, seen) = stub();
    let dir = std::env::temp_dir().join(format!("jf-tmdb-cache-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let t = client(&base, "good-key", Some(dir.clone()));
    let first = t.credits(9100001).unwrap();
    let n = seen.lock().unwrap().len();
    let second = t.credits(9100001).unwrap();
    assert_eq!(first, second);
    assert_eq!(seen.lock().unwrap().len(), n, "second call served from the disk cache");
    for e in walk(&dir) {
        assert!(!e.contains("good-key"), "key leaked into cache file name {e}");
        assert!(
            !std::fs::read_to_string(&e).unwrap_or_default().contains("good-key"),
            "key leaked into cache file {e}"
        );
    }
}
fn walk(d: &std::path::Path) -> Vec<String> {
    let mut v = vec![];
    if let Ok(rd) = std::fs::read_dir(d) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                v.extend(walk(&p));
            } else {
                v.push(p.to_string_lossy().into_owned());
            }
        }
    }
    v
}

#[test]
fn matching_server_items() {
    let srv = |n: &str, y: i32, k: ItemKind| Item {
        id: n.into(),
        name: n.into(),
        production_year: Some(y),
        kind: k,
        ..Default::default()
    };
    let server = vec![
        srv("Star Drift - Shadow Accord", 2002, ItemKind::Movie),
        srv("Star Drift - Beacon", 1977, ItemKind::Movie),
        srv("Harbour Lights", 2015, ItemKind::Series),
    ];
    let (base, _) = stub();
    let c = client(&base, "good-key", None).credits(9100001).unwrap();
    let get = |t: &str| c.iter().find(|x| x.title == t).unwrap();
    assert_eq!(
        on_server(get("Star Drift: Shadow Accord"), &server).map(|i| i.id.as_str()),
        Some("Star Drift - Shadow Accord")
    );
    assert!(on_server(get("Star Drift - Beacon"), &server).is_some());
    assert!(on_server(get("Quiet Orchard"), &server).is_none());
    assert!(on_server(get("Harbour Lights"), &server).is_none(), "year differs (2016 vs 2015)");
    assert_eq!(Tmdb::poster_url("/ember.jpg"), "https://image.tmdb.org/t/p/w342/ember.jpg");
}
