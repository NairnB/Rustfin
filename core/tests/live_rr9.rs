//! RR9 live tests against the throwaway server ONLY (`JF_TEST_URL`; admin tester / test-pass-1). Skipped without it.
//! The trigger round trip puts the task's original trigger list back, even when an assert fails.
use jf_core::api::*;

fn url() -> Option<String> {
    let u = std::env::var("JF_TEST_URL").ok();
    if u.is_none() {
        eprintln!("JF_TEST_URL unset: live test skipped");
    }
    u
}

fn signed(base: &str, user: &str, pass: &str) -> Client {
    let dev = DeviceInfo { name: "live-rr9".into(), id: "live-rr9-device".into(), version: "0.1.0".into() };
    let (mut c, _) = Client::connect(base, dev).unwrap();
    c.sign_in(user, pass).unwrap();
    c
}

/// Puts a task's original trigger list back when dropped.
struct Restore {
    client: Client,
    id: String,
    triggers: Vec<gen_admin::TaskTriggerInfo>,
}

impl Drop for Restore {
    fn drop(&mut self) {
        let _ = self.client.set_task_triggers(&self.id, &self.triggers);
    }
}

fn types(t: &gen_admin::TaskInfo) -> Vec<(Option<String>, Option<i64>)> {
    t.triggers.clone().unwrap_or_default().into_iter().map(|x| (x.type_, x.interval_ticks)).collect()
}

#[test]
fn trigger_added_then_removed_leaves_the_list_as_it_was() {
    let Some(u) = url() else { return };
    let admin = signed(&u, "tester", "test-pass-1");
    let task = admin
        .scheduled_tasks()
        .unwrap()
        .into_iter()
        .find(|t| t.name.as_deref() == Some("Clean Log Directory"))
        .expect("Clean Log Directory task");
    let id = task.id.clone().unwrap();
    let before = task.triggers.clone().unwrap_or_default();
    let _guard = Restore { client: admin.clone(), id: id.clone(), triggers: before.clone() };

    let mut added = before.clone();
    added.push(gen_admin::TaskTriggerInfo {
        type_: Some("IntervalTrigger".into()),
        interval_ticks: Some(7 * 3600 * 10_000_000),
        ..Default::default()
    });
    admin.set_task_triggers(&id, &added).unwrap();
    let now = admin.scheduled_task(&id).unwrap();
    assert_eq!(now.triggers.clone().unwrap_or_default().len(), before.len() + 1);
    assert!(types(&now).contains(&(Some("IntervalTrigger".into()), Some(7 * 3600 * 10_000_000))));

    admin.set_task_triggers(&id, &before).unwrap();
    let back = admin.scheduled_task(&id).unwrap();
    assert_eq!(types(&back), types(&task));
}

#[test]
fn configuration_pages_list_loads_for_an_admin() {
    let Some(u) = url() else { return };
    let admin = signed(&u, "tester", "test-pass-1");
    let pages = admin.configuration_pages().unwrap();
    // The throwaway server may have no plugin page; every entry that has a plugin must yield a clean URL.
    for p in pages.iter().filter(|p| p.plugin_id.is_some()) {
        assert!(admin::plugin_settings_url(&u, &p.name).contains("configurationpage?name="));
    }
}
