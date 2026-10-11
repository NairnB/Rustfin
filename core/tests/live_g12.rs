//! G12 live tests against the throwaway server ONLY (`JF_TEST_URL`). Password change runs on a temporary user
//! (`tmp-g12-<random>`, deleted even when the test fails) — never on viewer, whose password other lanes sign in
//! with at the same time; Quick Connect authorisation approves a code from a second client.
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

/// Deletes the temporary user when the test ends, pass or fail.
struct TempUser {
    admin: Client,
    id: String,
}
impl Drop for TempUser {
    fn drop(&mut self) {
        let _ = self.admin.delete_user(&self.id);
    }
}

// P-USER-1: a user changes their own password (a temporary user, never viewer).
#[test]
fn change_password_round_trip() {
    let Some(u) = url() else { return };
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let name = format!("tmp-g12-{:x}", n % 0xffff_ffff);
    let admin = signed(&u, "tester", "test-pass-1");
    let id = admin.create_user(&name, "tmp-pass-1").unwrap().id;
    let _guard = TempUser { admin, id };
    let c = signed(&u, &name, "tmp-pass-1");
    let uid = c.session.as_ref().unwrap().user_id.clone();

    // Wrong current password is refused (any error; the exact code varies by server).
    assert!(c.change_password(&uid, "wrong", "tmp-pass-2").is_err());

    c.change_password(&uid, "tmp-pass-1", "tmp-pass-2").unwrap();

    // Old password no longer signs in; the new one does.
    let (mut c2, _) = Client::connect(&u, dev()).unwrap();
    assert_eq!(c2.sign_in(&name, "tmp-pass-1").unwrap_err(), ApiError::Unauthorized);
    c2.sign_in(&name, "tmp-pass-2").unwrap();
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
