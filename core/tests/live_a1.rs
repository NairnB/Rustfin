//! A1 live tests against the throwaway server ONLY (`JF_TEST_URL`, e.g. http://jf-test:8096; admin user
//! tester / test-pass-1, non-admin viewer / view-pass-1). Skipped when JF_TEST_URL is unset. Run with
//! --test-threads=1.
use jf_core::api::*;

fn url() -> Option<String> {
    let u = std::env::var("JF_TEST_URL").ok();
    if u.is_none() {
        eprintln!("JF_TEST_URL unset: live test skipped");
    }
    u
}

fn dev() -> DeviceInfo {
    DeviceInfo { name: "live-a1".into(), id: "live-a1-device".into(), version: "0.1.0".into() }
}

fn signed(base: &str, user: &str, pass: &str) -> Client {
    let (mut c, _) = Client::connect(base, dev()).unwrap();
    c.sign_in(user, pass).unwrap();
    c
}

#[test]
fn system_info_sessions_tasks_decode() {
    let Some(u) = url() else { return };
    let c = signed(&u, "tester", "test-pass-1");
    let info = c.system_info().unwrap();
    assert!(info.version.as_deref().is_some_and(|v| v.starts_with("10.10")), "{info:?}");
    assert!(info.server_name.as_deref().is_some_and(|s| !s.is_empty()));
    let sessions = c.sessions().unwrap();
    assert!(!sessions.is_empty());
    let tasks = c.scheduled_tasks().unwrap();
    assert!(tasks.iter().any(|t| t.name.as_deref() == Some("Scan Media Library")));
}

#[test]
fn activity_log_decodes() {
    let Some(u) = url() else { return };
    let c = signed(&u, "tester", "test-pass-1");
    let page = c.activity_log(0, 10).unwrap();
    assert!(!page.items.is_empty());
    assert!(page.total_record_count > 0);
    assert!(page.items.iter().all(|e| !e.date.is_empty()));
}

#[test]
fn run_scan_media_library_to_completion() {
    let Some(u) = url() else { return };
    let c = signed(&u, "tester", "test-pass-1");
    let task = c
        .scheduled_tasks()
        .unwrap()
        .into_iter()
        .find(|t| t.name.as_deref() == Some("Scan Media Library"))
        .expect("scan task present");
    let id = task.id.clone().expect("scan task has an id");
    c.run_task(&id).unwrap();

    // The task flips to Running then back to Idle with a Completed result; a small library scans quickly.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(90);
    let mut saw_running = false;
    let result_status = loop {
        std::thread::sleep(std::time::Duration::from_millis(300));
        let tasks = c.scheduled_tasks().unwrap();
        let t = tasks.iter().find(|t| t.id.as_deref() == Some(id.as_str())).expect("task still listed");
        let state = t.state.clone();
        if state == "Running" {
            saw_running = true;
        } else if saw_running {
            break t.last_execution_result.as_ref().map(|r| r.status.clone()).unwrap_or_default();
        }
        if std::time::Instant::now() > deadline {
            panic!("scan task never ran to completion (state={state})");
        }
    };
    assert_eq!(result_status, "Completed", "scan task ended with {result_status}");
}

#[test]
fn viewer_admin_calls_are_forbidden() {
    let Some(u) = url() else { return };
    let c = signed(&u, "viewer", "view-pass-1");
    assert_eq!(c.scheduled_tasks().unwrap_err(), ApiError::Forbidden);
    assert_eq!(c.activity_log(0, 10).unwrap_err(), ApiError::Forbidden);
    assert_eq!(c.scan_all_libraries().unwrap_err(), ApiError::Forbidden);
    assert_eq!(c.run_task("does-not-matter").unwrap_err(), ApiError::Forbidden);
}
