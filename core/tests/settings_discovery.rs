//! C2 rules: settings (R-SET-1, R-SET-4) and discovery (R-DISC-*). Lead's tests; builders never edit.
use jf_core::api::Session;
use jf_core::discovery::*;
use jf_core::platform::Storage;
use jf_core::settings::*;
use std::time::{Duration, Instant};

fn tmpdir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("jf-test-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}
fn entry(id: &str, tok: Option<&str>) -> ServerEntry {
    ServerEntry {
        id: id.into(),
        name: format!("Server {id}"),
        address: format!("http://{id}:8096"),
        session: tok.map(|t| Session { user_id: "u".into(), user_name: "tester".into(), token: t.into() }),
    }
}

#[test]
fn device_id_is_32_hex_and_random() {
    let a = new_device_id();
    let b = new_device_id();
    assert_eq!(a.len(), 32);
    assert!(a.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    assert_ne!(a, b);
}

#[test]
fn fresh_load_and_round_trip() {
    let st = MemStorage::default();
    let mut s = Settings::load(&st);
    assert_eq!(s.device_id.len(), 32);
    assert!(s.servers.is_empty() && s.last().is_none());
    assert!(st.read(SETTINGS_KEY).is_none(), "load must not write");
    s.remember(entry("a", Some("t1")));
    s.save(&st).unwrap();
    let back = Settings::load(&st);
    assert_eq!(back, s);
    assert_eq!(back.last().unwrap().id, "a");
}

#[test]
fn corrupt_file_gives_fresh_settings() {
    let st = MemStorage::default();
    st.write(SETTINGS_KEY, b"{not json").unwrap();
    let s = Settings::load(&st);
    assert!(s.servers.is_empty());
    assert_eq!(s.device_id.len(), 32);
}

#[test]
fn remember_updates_in_place_and_forget_session() {
    let mut s = Settings::default();
    s.remember(entry("a", Some("t1")));
    s.remember(entry("b", None));
    s.remember(entry("a", Some("t2")));
    assert_eq!(s.servers.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), ["a", "b"]);
    assert_eq!(s.last().unwrap().session.as_ref().unwrap().token, "t2");
    s.forget_session("a");
    assert!(s.servers[0].session.is_none());
    assert_eq!(s.servers.len(), 2);
}

#[test]
fn file_storage_atomic_and_cache_dir() {
    let d = tmpdir("fs");
    let st = FileStorage::new(d.clone()).unwrap();
    assert!(st.read("x.json").is_none());
    st.write("x.json", b"one").unwrap();
    st.write("x.json", b"two").unwrap();
    assert_eq!(st.read("x.json").unwrap(), b"two");
    let leftovers: Vec<_> = std::fs::read_dir(&d)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(leftovers, ["x.json"], "no temp files left behind");
    assert!(st.cache_dir().starts_with(&d));
    assert!(st.cache_dir().is_dir());
}

#[test]
fn parse_discovery_reply() {
    let r = br#"{"Address":"http://172.40.0.3:8096","Id":"e5a0d153","Name":"Test","EndpointAddress":null}"#;
    let s = parse_reply(r).unwrap();
    assert_eq!((s.id.as_str(), s.name.as_str(), s.address.as_str()), ("e5a0d153", "Test", "http://172.40.0.3:8096"));
    assert!(parse_reply(b"hello").is_none());
    assert!(parse_reply(br#"{"Name":"x"}"#).is_none());
}

// Live: unicast to the throwaway server (JF_TEST_URL host).
#[test]
fn discover_test_server_unicast() {
    let Ok(u) = std::env::var("JF_TEST_URL") else {
        eprintln!("JF_TEST_URL unset: skipped");
        return;
    };
    use std::net::ToSocketAddrs;
    let host = u.trim_start_matches("http://").split(':').next().unwrap().to_string();
    let addr = (host.as_str(), DISCOVERY_PORT).to_socket_addrs().unwrap().next().unwrap();
    let found = discover_at(addr, Duration::from_millis(1500));
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].address.contains(":8096"));
}

#[test]
fn discover_never_errors_and_respects_wait() {
    let t = Instant::now();
    let _ = discover(Duration::from_millis(300));
    assert!(t.elapsed() < Duration::from_millis(1500));
}
