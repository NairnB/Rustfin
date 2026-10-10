//! G13 live tests (sign-in extras, search extras). Read-only against the throwaway server; skipped without `JF_TEST_URL`.
//! The forgot-password request is for a made-up user and never redeems a PIN.
use jf_core::api::*;

fn url() -> Option<String> {
    let u = std::env::var("JF_TEST_URL").ok();
    if u.is_none() {
        eprintln!("JF_TEST_URL unset: live test skipped");
    }
    u
}
fn dev() -> DeviceInfo {
    DeviceInfo { name: "live-g13".into(), id: "live-g13-device".into(), version: "0.1.0".into() }
}

fn signed_in(base: &str) -> Client {
    let (mut c, _) = Client::connect(base, dev()).unwrap();
    c.sign_in("viewer", "view-pass-1").unwrap();
    c
}

#[test]
fn branding_decodes() {
    let Some(u) = url() else { return };
    let (c, _) = Client::connect(&u, dev()).unwrap();
    let b = c.branding().unwrap();
    // The disclaimer may be empty; it must decode either way.
    assert!(b.login_disclaimer.as_deref().unwrap_or("").len() < 100_000);
}

#[test]
fn forgot_password_for_unknown_user_answers() {
    let Some(u) = url() else { return };
    let (c, _) = Client::connect(&u, dev()).unwrap();
    let r = c.forgot_password("no-such-user-g13").unwrap();
    assert!(!r.message().is_empty());
    assert!(!matches!(r.action, ForgotAction::Pin), "an unknown user must not get a PIN: {r:?}");
}

#[test]
fn wrong_pin_is_not_a_success() {
    let Some(u) = url() else { return };
    let (c, _) = Client::connect(&u, dev()).unwrap();
    let r = c.forgot_password_pin("000000-not-a-real-pin");
    // Either the server answers "not successful" or it refuses; it never resets anyone.
    if let Ok(ok) = r {
        assert!(!ok.success);
    }
}

#[test]
fn people_collections_and_suggestions() {
    let Some(u) = url() else { return };
    let c = signed_in(&u);
    let coll = c.search("Star Drift").unwrap();
    assert!(
        coll.iter().any(|i| i.kind == ItemKind::BoxSet),
        "collection in search: {:?}",
        coll.iter().map(|i| &i.name).collect::<Vec<_>>()
    );
    // People: any name from a film's cast finds that person.
    let films = c.search("Ember Rising").unwrap();
    let film = c.item(&films.iter().find(|i| i.kind == ItemKind::Movie).unwrap().id).unwrap();
    if let Some(p) = film.people.first() {
        let first = p.name.split_whitespace().next().unwrap().to_string();
        let found = c.search_people(&first).unwrap();
        assert!(found.iter().any(|x| x.id == p.id), "person {} found by {first}", p.name);
    }
    assert!(!c.suggestions(12).unwrap().is_empty());
}

#[test]
fn forgot_messages() {
    let pin =
        ForgotPassword { action: ForgotAction::Pin, pin_file: Some("/config/x.json".into()), ..Default::default() };
    assert!(pin.message().contains("/config/x.json"));
    assert!(ForgotPassword::default().message().contains("administrator"));
    let r: ForgotPassword = serde_json::from_str(r#"{"Action":"PinCode","PinFile":"f"}"#).unwrap();
    assert_eq!(r.action, ForgotAction::Pin);
}
