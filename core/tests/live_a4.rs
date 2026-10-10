//! A4 live tests against the throwaway server ONLY (`JF_TEST_URL`; admin tester / test-pass-1, non-admin
//! viewer / view-pass-1). Skipped when JF_TEST_URL is unset. Run with --test-threads=1. Each write changes one
//! value and puts the server's own copy back (a guard restores it even when the test fails). Networking is only read.
use jf_core::api::*;
use serde_json::{json, Value};

fn url() -> Option<String> {
    let u = std::env::var("JF_TEST_URL").ok();
    if u.is_none() {
        eprintln!("JF_TEST_URL unset: live test skipped");
    }
    u
}

fn signed(base: &str, user: &str, pass: &str) -> Client {
    let dev = DeviceInfo { name: "live-a4".into(), id: format!("live-a4-{user}"), version: "0.1.0".into() };
    let (mut c, _) = Client::connect(base, dev).unwrap();
    c.sign_in(user, pass).unwrap();
    c
}

/// Posts the saved section back on drop, whatever the test did.
struct Restore {
    c: Client,
    section: Option<&'static str>,
    saved: Value,
}

impl Restore {
    fn new(c: &Client, section: Option<&'static str>) -> Restore {
        let saved = read(c, section);
        Restore { c: c.clone(), section, saved }
    }
}

impl Drop for Restore {
    fn drop(&mut self) {
        let r = match self.section {
            Some(s) => self.c.set_named_configuration(s, &self.saved),
            None => self.c.set_system_configuration(&self.saved),
        };
        if let Err(e) = r {
            eprintln!("RESTORE FAILED for {:?}: {e}", self.section);
        }
    }
}

fn read(c: &Client, section: Option<&str>) -> Value {
    match section {
        Some(s) => c.named_configuration(s).unwrap(),
        None => c.system_configuration().unwrap(),
    }
}

/// What the pages do on Save: read again, change one field, post the whole thing back.
fn edit(c: &Client, section: Option<&str>, key: &str, value: Value) {
    let mut cfg = read(c, section);
    cfg[key] = value;
    match section {
        Some(s) => c.set_named_configuration(s, &cfg).unwrap(),
        None => c.set_system_configuration(&cfg).unwrap(),
    }
}

#[test]
fn configs_decode_and_viewer_cannot_write() {
    let Some(u) = url() else { return };
    let c = signed(&u, "tester", "test-pass-1");
    let sys = read(&c, None);
    for k in [
        "ServerName",
        "UICulture",
        "QuickConnectAvailable",
        "LibraryMonitorDelay",
        "MinResumePct",
        "MaxResumePct",
        "MinResumeDurationSeconds",
        "RemoteClientBitrateLimit",
    ] {
        assert!(sys.get(k).is_some(), "system config lacks {k}");
    }
    assert!(sys["TrickplayOptions"].get("Interval").is_some());
    let enc = read(&c, Some("encoding"));
    for k in [
        "HardwareAccelerationType",
        "VaapiDevice",
        "HardwareDecodingCodecs",
        "EncodingThreadCount",
        "EnableThrottling",
        "TonemappingAlgorithm",
    ] {
        assert!(enc.get(k).is_some(), "encoding lacks {k}");
    }
    let net = read(&c, Some("network"));
    for k in ["BaseUrl", "InternalHttpPort", "EnableHttps", "EnableRemoteAccess", "LocalNetworkSubnets", "KnownProxies"]
    {
        assert!(net.get(k).is_some(), "network lacks {k}");
    }
    assert!(read(&c, Some("branding")).get("SplashscreenEnabled").is_some());
    assert!(c.system_info().unwrap().cache_path.is_some());

    let v = signed(&u, "viewer", "view-pass-1");
    assert!(matches!(v.set_system_configuration(&sys), Err(ApiError::Forbidden)));
    assert!(matches!(v.set_named_configuration("encoding", &enc), Err(ApiError::Forbidden)));
    assert!(matches!(v.set_named_configuration("network", &net), Err(ApiError::Forbidden)));
    assert!(matches!(
        v.set_named_configuration("branding", &json!({"SplashscreenEnabled": false})),
        Err(ApiError::Forbidden)
    ));
}

#[test]
fn server_name_round_trips() {
    let Some(u) = url() else { return };
    let c = signed(&u, "tester", "test-pass-1");
    let before = read(&c, None);
    let _g = Restore::new(&c, None);
    edit(&c, None, "ServerName", json!("tmp-a4"));
    assert_eq!(c.system_info().unwrap().server_name.as_deref(), Some("tmp-a4"));
    let mid = read(&c, None);
    assert_eq!(mid["ServerName"], "tmp-a4");
    // Nothing else moved.
    let (mut a, mut b) = (before.clone(), mid);
    a["ServerName"] = json!(null);
    b["ServerName"] = json!(null);
    assert_eq!(a, b);
    edit(&c, None, "ServerName", before["ServerName"].clone());
    assert_eq!(read(&c, None), before);
}

#[test]
fn branding_disclaimer_set_and_cleared() {
    let Some(u) = url() else { return };
    let c = signed(&u, "tester", "test-pass-1");
    let before = read(&c, Some("branding"));
    let _g = Restore::new(&c, Some("branding"));
    edit(&c, Some("branding"), "LoginDisclaimer", json!("tmp-a4 disclaimer"));
    // The public sign-in page reads it from /Branding/Configuration.
    assert_eq!(read(&c, Some("branding"))["LoginDisclaimer"], "tmp-a4 disclaimer");
    edit(&c, Some("branding"), "LoginDisclaimer", json!(""));
    let after = read(&c, Some("branding"));
    assert!(after.get("LoginDisclaimer").and_then(Value::as_str).unwrap_or("").is_empty());
    assert_eq!(after.get("SplashscreenEnabled"), before.get("SplashscreenEnabled"));
}

#[test]
fn resume_values_round_trip_unchanged() {
    let Some(u) = url() else { return };
    let c = signed(&u, "tester", "test-pass-1");
    let before = read(&c, None);
    let _g = Restore::new(&c, None);
    // Save with nothing edited (what the Resume page posts): the three values come back identical.
    let cfg = read(&c, None);
    c.set_system_configuration(&cfg).unwrap();
    let after = read(&c, None);
    for k in ["MinResumePct", "MaxResumePct", "MinResumeDurationSeconds"] {
        assert_eq!(after[k], before[k], "{k}");
    }
    // Change one and put it back; the test server's MinResumeDurationSeconds (1) must end as it began.
    edit(&c, None, "MaxResumePct", json!(95));
    assert_eq!(read(&c, None)["MaxResumePct"], 95);
    edit(&c, None, "MaxResumePct", before["MaxResumePct"].clone());
    assert_eq!(read(&c, None), before);
    assert_eq!(read(&c, None)["MinResumeDurationSeconds"], before["MinResumeDurationSeconds"]);
}

#[test]
fn encoding_trickplay_streaming_round_trip_and_network_is_read_only() {
    let Some(u) = url() else { return };
    let c = signed(&u, "tester", "test-pass-1");
    let enc = read(&c, Some("encoding"));
    let sys = read(&c, None);
    let _g1 = Restore::new(&c, Some("encoding"));
    let _g2 = Restore::new(&c, None);
    edit(&c, Some("encoding"), "EnableThrottling", json!(!enc["EnableThrottling"].as_bool().unwrap()));
    assert_ne!(read(&c, Some("encoding"))["EnableThrottling"], enc["EnableThrottling"]);
    edit(&c, Some("encoding"), "EnableThrottling", enc["EnableThrottling"].clone());
    assert_eq!(read(&c, Some("encoding")), enc);

    let mut cfg = read(&c, None);
    cfg["TrickplayOptions"]["Interval"] = json!(12000);
    cfg["RemoteClientBitrateLimit"] = json!(20_000_000);
    c.set_system_configuration(&cfg).unwrap();
    let mid = read(&c, None);
    assert_eq!(mid["TrickplayOptions"]["Interval"], 12000);
    assert_eq!(mid["RemoteClientBitrateLimit"], 20_000_000);
    c.set_system_configuration(&sys).unwrap();
    assert_eq!(read(&c, None), sys);

    // Networking: read only, twice, identical.
    assert_eq!(read(&c, Some("network")), read(&c, Some("network")));
}
