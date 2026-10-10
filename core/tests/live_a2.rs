//! A2 live tests against the throwaway server ONLY (`JF_TEST_URL`; admin tester / test-pass-1, non-admin
//! viewer / view-pass-1). Skipped when JF_TEST_URL is unset. Run with --test-threads=1. Every write goes to a
//! temporary user `tmp-a2-<random>` that is deleted at the end; `tester` and `viewer` are never changed.
use jf_core::api::gen_admin::AccessSchedule;
use jf_core::api::*;

fn url() -> Option<String> {
    let u = std::env::var("JF_TEST_URL").ok();
    if u.is_none() {
        eprintln!("JF_TEST_URL unset: live test skipped");
    }
    u
}

fn dev() -> DeviceInfo {
    DeviceInfo { name: "live-a2".into(), id: "live-a2-device".into(), version: "0.1.0".into() }
}

fn signed(base: &str, user: &str, pass: &str) -> Client {
    let (mut c, _) = Client::connect(base, dev()).unwrap();
    c.sign_in(user, pass).unwrap();
    c
}

fn random_suffix() -> String {
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    format!("{:x}", n % 0xffff_ffff)
}

#[test]
fn users_list_and_ratings_and_folders_decode() {
    let Some(u) = url() else { return };
    let c = signed(&u, "tester", "test-pass-1");
    let users = c.users().unwrap();
    assert!(users.iter().any(|x| x.name.as_deref() == Some("tester")));
    assert!(users.iter().any(|x| x.name.as_deref() == Some("viewer")));
    let ratings = c.parental_ratings().unwrap();
    assert!(ratings.iter().any(|r| r.name == "PG" && r.value == Some(8)), "{ratings:?}");
    let folders = c.virtual_folders().unwrap();
    assert!(folders.iter().any(|f| f.name.as_deref() == Some("Movies") && f.item_id.is_some()));
}

#[test]
fn temp_user_policy_access_rating_password_and_delete() {
    let Some(u) = url() else { return };
    let c = signed(&u, "tester", "test-pass-1");
    let name = format!("tmp-a2-{}", random_suffix());
    let created = c.create_user(&name, "tmp-pass-1").unwrap();
    let id = created.id.clone();
    assert_eq!(created.name.as_deref(), Some(name.as_str()));

    // Run the body so the user is always deleted, even when an assert fails.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // Policy flags.
        let mut policy = c.user(&id).unwrap().policy.expect("policy");
        assert!(!policy.is_administrator);
        policy.is_disabled = true;
        policy.enable_remote_access = false;
        policy.enable_content_deletion = true;
        policy.enable_media_playback = false;
        policy.enable_all_devices = false;
        c.set_user_policy(&id, &policy).unwrap();
        let p = c.user(&id).unwrap().policy.unwrap();
        assert!(
            !p.enable_all_devices
                && p.is_disabled
                && !p.enable_remote_access
                && p.enable_content_deletion
                && !p.enable_media_playback
        );

        // Library access, rating, blocked tag and a schedule.
        let movies = c
            .virtual_folders()
            .unwrap()
            .into_iter()
            .find(|f| f.name.as_deref() == Some("Movies"))
            .and_then(|f| f.item_id)
            .expect("Movies library");
        let mut policy = p;
        policy.is_disabled = false;
        policy.enable_remote_access = true; // the test client is not on the server's LAN
        policy.enable_media_playback = true;
        policy.enable_all_folders = false;
        policy.enabled_folders = Some(vec![movies.clone()]);
        policy.max_parental_rating = Some(8);
        policy.blocked_tags = Some(vec!["nope".into()]);
        policy.enable_all_devices = true; // a device list of none would lock the test client out
        policy.access_schedules = Some(vec![AccessSchedule {
            user_id: id.clone(),
            day_of_week: "Everyday".into(),
            start_hour: 0.0,
            end_hour: 24.0,
            ..Default::default()
        }]);
        c.set_user_policy(&id, &policy).unwrap();
        let p = c.user(&id).unwrap().policy.unwrap();
        assert!(!p.enable_all_folders && p.enable_all_devices);
        assert_eq!(p.enabled_folders, Some(vec![movies]));
        assert_eq!(p.max_parental_rating, Some(8));
        assert_eq!(p.blocked_tags, Some(vec!["nope".to_string()]));
        assert_eq!(p.access_schedules.as_ref().map(|s| s.len()), Some(1));

        // Rename, then password: the temp user can sign in with the new password and not the old one.
        let mut dto = c.user(&id).unwrap();
        dto.name = Some(format!("{name}-r"));
        c.update_user(&id, &dto).unwrap();
        assert_eq!(c.user(&id).unwrap().name, dto.name);
        c.set_user_password(&id, "tmp-pass-2").unwrap();
        let (mut other, _) = Client::connect(&url().unwrap(), dev()).unwrap();
        assert!(other.sign_in(dto.name.as_deref().unwrap(), "tmp-pass-1").is_err());
        other.sign_in(dto.name.as_deref().unwrap(), "tmp-pass-2").unwrap();
        // The new user is not an administrator.
        assert_eq!(other.create_user("tmp-a2-nope", "x").unwrap_err(), ApiError::Forbidden);
        // Reset clears the password.
        c.reset_user_password(&id).unwrap();
        assert!(!c.user(&id).unwrap().has_password);
    }));

    c.delete_user(&id).unwrap();
    assert!(!c.users().unwrap().iter().any(|x| x.id == id));
    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
}

#[test]
fn viewer_user_admin_calls_are_forbidden() {
    let Some(u) = url() else { return };
    let c = signed(&u, "viewer", "view-pass-1");
    // GET /Users is open to any signed-in user on 10.10.7; only the writes and the folders are admin-gated.
    assert_eq!(c.virtual_folders().unwrap_err(), ApiError::Forbidden);
    assert_eq!(c.create_user("tmp-a2-nope", "x").unwrap_err(), ApiError::Forbidden);
}
