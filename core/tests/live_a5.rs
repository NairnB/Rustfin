//! A5 live tests against the throwaway server ONLY (`JF_TEST_URL`; admin tester / test-pass-1, non-admin
//! viewer / view-pass-1). Skipped when JF_TEST_URL is unset. Run with --test-threads=1. Plugins are only read:
//! nothing is installed, uninstalled or switched here. Keys are never printed.
use jf_core::api::admin::{mask_key, newest_first};
use jf_core::api::*;

fn url() -> Option<String> {
    let u = std::env::var("JF_TEST_URL").ok();
    if u.is_none() {
        eprintln!("JF_TEST_URL unset: live test skipped");
    }
    u
}

fn signed(base: &str, user: &str, pass: &str, dev_id: &str) -> Client {
    let dev = DeviceInfo { name: "live-a5".into(), id: dev_id.into(), version: "0.1.0".into() };
    let (mut c, _) = Client::connect(base, dev).unwrap();
    c.sign_in(user, pass).unwrap();
    c
}

/// Revokes every key whose app name starts with `tmp-a5`, at the start and again on drop.
struct KeyGuard(Client);

impl KeyGuard {
    fn new(c: &Client) -> KeyGuard {
        let g = KeyGuard(c.clone());
        g.sweep();
        g
    }
    fn sweep(&self) {
        for k in self.0.api_keys().unwrap_or_default() {
            if k.app_name.as_deref().is_some_and(|a| a.starts_with("tmp-a5")) {
                if let Some(t) = &k.access_token {
                    let _ = self.0.revoke_api_key(t);
                }
            }
        }
    }
}

impl Drop for KeyGuard {
    fn drop(&mut self) {
        self.sweep();
    }
}

/// Puts a device's custom name back on drop.
struct NameGuard {
    c: Client,
    id: String,
    name: String,
}

impl Drop for NameGuard {
    fn drop(&mut self) {
        if let Err(e) = self.c.rename_device(&self.id, &self.name) {
            eprintln!("RESTORE FAILED for device name: {e}");
        }
    }
}

#[test]
fn keys_create_and_revoke() {
    let Some(u) = url() else { return };
    let c = signed(&u, "tester", "test-pass-1", "live-a5-tester");
    let _g = KeyGuard::new(&c);
    c.create_api_key("tmp-a5").unwrap();
    let keys = c.api_keys().unwrap();
    let k = keys.iter().find(|k| k.app_name.as_deref() == Some("tmp-a5")).expect("new key listed");
    let token = k.access_token.clone().unwrap();
    assert!(token.len() > 8);
    let masked = mask_key(&token);
    assert!(masked.ends_with(&token[token.len() - 4..]));
    assert!(!masked.contains(&token[..token.len() - 4]));
    c.revoke_api_key(&token).unwrap();
    let after = c.api_keys().unwrap();
    assert!(!after.iter().any(|k| k.app_name.as_deref() == Some("tmp-a5")), "key still listed");
}

#[test]
fn devices_list_and_rename_round_trip() {
    let Some(u) = url() else { return };
    let admin = signed(&u, "tester", "test-pass-1", "live-a5-tester");
    // A second client with its own device id: this is the one that gets renamed.
    let _other = signed(&u, "viewer", "view-pass-1", "live-a5-viewer");
    let devices = admin.devices().unwrap();
    let d = devices.iter().find(|d| d.id.as_deref() == Some("live-a5-viewer")).expect("viewer device listed");
    assert!(d.app_name.is_some() || d.name.is_some());
    let id = d.id.clone().unwrap();
    let original = d.custom_name.clone().unwrap_or_default();
    let _g = NameGuard { c: admin.clone(), id: id.clone(), name: original.clone() };

    admin.rename_device(&id, "tmp-a5").unwrap();
    let now = admin.devices().unwrap();
    let d = now.iter().find(|d| d.id.as_deref() == Some(id.as_str())).unwrap();
    assert_eq!(d.custom_name.as_deref(), Some("tmp-a5"));

    admin.rename_device(&id, &original).unwrap();
    let back = admin.devices().unwrap();
    let d = back.iter().find(|d| d.id.as_deref() == Some(id.as_str())).unwrap();
    assert_eq!(d.custom_name.clone().unwrap_or_default(), original);
}

#[test]
fn logs_list_decodes_and_one_file_reads() {
    let Some(u) = url() else { return };
    let c = signed(&u, "tester", "test-pass-1", "live-a5-tester");
    let logs = c.log_files().unwrap();
    assert!(!logs.is_empty(), "the server has at least one log file");
    let first = logs.iter().max_by(|a, b| a.date_modified.cmp(&b.date_modified)).unwrap();
    assert!(!first.name.is_empty());
    let text = c.log_text(&first.name).unwrap();
    assert!(!text.is_empty());
    let lines = newest_first(&text, 50);
    assert!(lines.len() <= 50);
}

#[test]
fn plugins_packages_repositories_decode() {
    let Some(u) = url() else { return };
    let c = signed(&u, "tester", "test-pass-1", "live-a5-tester");
    let plugins = c.plugins().unwrap();
    for p in &plugins {
        assert!(!p.name.is_empty());
        assert!(!p.version.is_empty());
    }
    let repos = c.repositories().unwrap();
    for r in &repos {
        assert!(r.url.as_deref().is_some_and(|u| !u.is_empty()));
    }
    // The catalogue needs the internet from the server; an empty list is fine, an error is not a decode failure.
    match c.packages() {
        Ok(p) => {
            for x in p {
                assert!(!x.name.is_empty());
            }
        }
        Err(ApiError::Network(_)) | Err(ApiError::Server(..)) => eprintln!("catalogue unreachable: skipped"),
        Err(e) => panic!("{e}"),
    }
}

#[test]
fn viewer_cannot_list_keys_or_logs() {
    let Some(u) = url() else { return };
    let v = signed(&u, "viewer", "view-pass-1", "live-a5-viewer");
    assert!(matches!(v.api_keys(), Err(ApiError::Forbidden)));
    assert!(matches!(v.log_files(), Err(ApiError::Forbidden)));
    assert!(matches!(v.plugins(), Err(ApiError::Forbidden)));
}

#[test]
fn mask_and_newest_first() {
    assert_eq!(mask_key("0123456789abcdef"), "••••••••cdef");
    assert_eq!(mask_key("abc"), "••••");
    assert_eq!(newest_first("a\nb\nc\n", 10), vec!["c", "b", "a"]);
    assert_eq!(newest_first("a\nb\nc", 2), vec!["c", "b"]);
    assert!(newest_first("", 5).is_empty());
}
