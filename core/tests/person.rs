//! F5b person-page model: the Acting / Crew tabs are driven by the TMDB helpers with the recorded fixtures,
//! never live calls. This pins the page's on-server marking for the "Ada Quill" fixture credits.
use jf_core::api::{Item, ItemKind};
use jf_core::tmdb::*;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;

fn fixture(n: &str) -> String {
    std::fs::read_to_string(format!("{}/tests/fixtures/tmdb/{n}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

/// A local stub serving the recorded TMDB answers (good-key only). No live TMDB.
fn stub() -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/3", l.local_addr().unwrap());
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
            let (code, body) = if !path.contains("api_key=good-key") {
                (401, r#"{"status_code":7}"#.to_string())
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
    base
}

fn server_film(name: &str, year: i32) -> Item {
    Item {
        id: name.into(),
        name: name.into(),
        production_year: Some(year),
        kind: ItemKind::Movie,
        ..Default::default()
    }
}

/// The three Star Drift films are on the server; the other fixture credits are not.
#[test]
fn person_page_marks_on_server_credits() {
    let base = stub();
    let mut t = Tmdb::new("good-key", None);
    t.base = base;
    let id = t.find_person("Ada Quill").unwrap().unwrap();
    let credits = t.credits(id).unwrap();
    let (acting, crew) = split_sorted(&credits);

    let server = vec![
        server_film("Star Drift - Ember Rising", 1999),
        server_film("Star Drift - Shadow Accord", 2002),
        server_film("Star Drift - Beacon", 1977),
    ];
    let on: Vec<&str> = acting.iter().filter(|c| on_server(c, &server).is_some()).map(|c| c.title.as_str()).collect();
    assert_eq!(
        on,
        ["Star Drift: Shadow Accord", "Star Drift - Ember Rising", "Star Drift - Beacon"],
        "newest-first order, all three on the server"
    );
    assert!(acting.iter().any(|c| c.title == "Quiet Orchard" && on_server(c, &server).is_none()));
    assert!(acting.iter().any(|c| c.title == "Harbour Lights" && on_server(c, &server).is_none()));
    assert_eq!(crew.len(), 1);
    assert_eq!(crew[0].role, "Director, Writer");
}
