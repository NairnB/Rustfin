//! G12 live tests against the throwaway server ONLY (`JF_TEST_URL`). Password change is a round trip that
//! puts the viewer's password back; Quick Connect authorisation approves a code from a second client.
//! Skipped without JF_TEST_URL. Run with --test-threads=1.
use jf_core::api::*;

fn url() -> Option<String> {
    let u = std::env::var("JF_TEST_URL").ok();
    if u.is_none() {
        eprintln!("JF_TEST_URL unset: live test skipped");
    }
    u
}
fn dev() -> DeviceInfo {
    DeviceInfo { name: "live-g12".into(), id: "live-g12-device".into(), version: "0.1.0".into() }
}
fn signed(base: &str, user: &str, pass: &str) -> Client {
    let (mut c, _) = Client::connect(base, dev()).unwrap();
    c.sign_in(user, pass).unwrap();
    c
}

// P-USER-1: the viewer changes their password and changes it back.
#[test]
fn change_password_round_trip() {
    let Some(u) = url() else { return };
    let c = signed(&u, "viewer", "view-pass-1");
    let uid = c.session.as_ref().unwrap().user_id.clone();

    // Wrong current password is refused (any error; the exact code varies by server).
    assert!(c.change_password(&uid, "wrong", "view-temp-1").is_err());

    c.change_password(&uid, "view-pass-1", "view-temp-1").unwrap();

    // Old password no longer signs in; the new one does.
    let (mut c2, _) = Client::connect(&u, dev()).unwrap();
    assert_eq!(c2.sign_in("viewer", "view-pass-1").unwrap_err(), ApiError::Unauthorized);
    c2.sign_in("viewer", "view-temp-1").unwrap();

    // Put it back.
    c2.change_password(&uid, "view-temp-1", "view-pass-1").unwrap();
    let (mut c3, _) = Client::connect(&u, dev()).unwrap();
    c3.sign_in("viewer", "view-pass-1").unwrap();
}

// P-AUTH-9 / P-USER-7: approve a code initiated by another device.
#[test]
fn quick_connect_authorize_approves_a_code() {
    let Some(u) = url() else { return };
    let c = signed(&u, "tester", "test-pass-1");
    let (other, _) = Client::connect(&u, dev()).unwrap();
    let st = other.quick_connect_initiate().unwrap();
    assert_eq!(st.code.len(), 6);
    assert!(!other.quick_connect_poll(&st.secret).unwrap());

    c.quick_connect_authorize(&st.code).unwrap();
    assert!(other.quick_connect_poll(&st.secret).unwrap());
}

// A code that does not exist is refused.
#[test]
fn quick_connect_authorize_bogus_code_is_refused() {
    let Some(u) = url() else { return };
    let c = signed(&u, "tester", "test-pass-1");
    assert!(c.quick_connect_authorize("000000").is_err());
}
