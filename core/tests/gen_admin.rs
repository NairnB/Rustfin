//! Live decode test for the generated admin DTOs (packet O1). Fetches real endpoints from the throwaway
//! server and decodes them into the types emitted by `tools/openapi/gen.py`. Skipped without `JF_TEST_URL`.
use jf_core::api::gen_admin::{ServerConfiguration, SystemInfo, UserDto};
use jf_core::api::{Client, DeviceInfo};

fn url() -> Option<String> {
    let u = std::env::var("JF_TEST_URL").ok();
    if u.is_none() {
        eprintln!("JF_TEST_URL unset: live test skipped");
    }
    u
}

fn dev() -> DeviceInfo {
    DeviceInfo { name: "o1-test".into(), id: "o1-test-device".into(), version: "0.1.0".into() }
}

fn signed(base: &str) -> Client {
    let (mut c, _) = Client::connect(base, dev()).unwrap();
    c.sign_in("tester", "test-pass-1").unwrap();
    c
}

fn get_json<T: serde::de::DeserializeOwned>(c: &Client, path: &str) -> T {
    let mut resp = ureq::get(&format!("{}{}", c.base, path)).header("Authorization", &c.auth_header()).call().unwrap();
    resp.body_mut().read_json().unwrap()
}

#[test]
fn decode_system_info() {
    let Some(u) = url() else { return };
    let c = signed(&u);
    let info: SystemInfo = get_json(&c, "/System/Info");
    assert!(info.version.as_deref().is_some_and(|v| v.starts_with("10.10")), "{info:?}");
    assert!(info.server_name.as_deref().is_some_and(|s| !s.is_empty()));
    assert!(info.id.as_deref().is_some_and(|s| !s.is_empty()));
}

#[test]
fn decode_users() {
    let Some(u) = url() else { return };
    let c = signed(&u);
    let users: Vec<UserDto> = get_json(&c, "/Users");
    let tester = users.iter().find(|x| x.name.as_deref() == Some("tester")).expect("tester user present");
    assert!(tester.has_password, "{tester:?}");
}

#[test]
fn decode_server_configuration() {
    let Some(u) = url() else { return };
    let c = signed(&u);
    let cfg: ServerConfiguration = get_json(&c, "/System/Configuration");
    assert_eq!(cfg.preferred_metadata_language, "en");
}
