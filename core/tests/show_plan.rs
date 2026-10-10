//! F1 rules (R-F1-1/2). Lead's tests; builders never edit.
use jf_core::api::*;
use jf_core::show::plan;

fn ep(s: i32, e: i32, played: bool, pos_secs: i64, len_secs: i64) -> Item {
    Item {
        id: format!("s{s}e{e}"),
        name: format!("Ep {e}"),
        kind: ItemKind::Episode,
        parent_index_number: Some(s),
        index_number: Some(e),
        run_time_ticks: Some(len_secs * 10_000_000),
        user_data: Some(UserData { played, playback_position_ticks: pos_secs * 10_000_000, ..Default::default() }),
        ..Default::default()
    }
}
/// Specials (S0 E1) + S1..S3 with 2 episodes each, 45 min long, every flag off.
fn show(mark: &[(i32, i32, bool, i64)]) -> Vec<Item> {
    let mut v = vec![ep(0, 1, false, 0, 2700)];
    for s in 1..=3 {
        for e in 1..=2 {
            v.push(ep(s, e, false, 0, 2700));
        }
    }
    for &(s, e, played, pos) in mark {
        let i = v.iter().position(|x| x.parent_index_number == Some(s) && x.index_number == Some(e)).unwrap();
        v[i] = ep(s, e, played, pos, 2700);
    }
    v.reverse(); // order must not matter
    v
}
fn label(p: &jf_core::show::ShowPlan) -> String {
    p.play.as_ref().map(|x| x.label.clone()).unwrap_or_default()
}

#[test]
fn seasons_listed_regular_then_specials() {
    assert_eq!(plan(&show(&[])).seasons, vec![1, 2, 3, 0]);
}

#[test]
fn never_watched_opens_season_1() {
    let p = plan(&show(&[]));
    assert_eq!((p.open_season, label(&p).as_str()), (1, "Play S1 E1"));
    assert_eq!(p.play.unwrap().episode_id, "s1e1");
}

#[test]
fn part_way_opens_the_next_episodes_season() {
    let p = plan(&show(&[(1, 1, true, 0), (1, 2, true, 0), (2, 1, true, 0)]));
    assert_eq!((p.open_season, label(&p).as_str()), (2, "Play S2 E2"));
}

#[test]
fn finished_a_season_and_a_newer_one_exists() {
    let p = plan(&show(&[(1, 1, true, 0), (1, 2, true, 0), (2, 1, true, 0), (2, 2, true, 0)]));
    assert_eq!((p.open_season, label(&p).as_str()), (3, "Play S3 E1"));
}

#[test]
fn resume_wins_and_says_minutes_left() {
    let p = plan(&show(&[(1, 1, true, 0), (2, 2, false, 33 * 60 + 10)]));
    assert_eq!((p.open_season, label(&p).as_str()), (2, "Resume S2 E2 · 12 min left"));
    assert!(p.play.unwrap().resume);
    let q = plan(&show(&[(3, 1, false, 2690)]));
    assert_eq!(label(&q), "Resume S3 E1 · 1 min left", "at least 1 minute");
}

#[test]
fn all_watched_opens_the_last_season_and_offers_s1e1() {
    let all: Vec<(i32, i32, bool, i64)> = (1..=3).flat_map(|s| (1..=2).map(move |e| (s, e, true, 0))).collect();
    let p = plan(&show(&all));
    assert_eq!((p.open_season, label(&p).as_str()), (3, "Play S1 E1"));
}

#[test]
fn specials_never_open_by_default() {
    let p = plan(&show(&[(0, 1, true, 0)]));
    assert_eq!(p.open_season, 1);
    let q = plan(&show(&[(0, 1, false, 600)]));
    assert_eq!(q.open_season, 1, "an in-progress special does not pick the season");
}

#[test]
fn empty_and_unnumbered() {
    let p = plan(&[]);
    assert_eq!((p.seasons.len(), p.play.is_none()), (0, true));
    let mut odd = ep(1, 1, false, 0, 60);
    odd.index_number = None;
    assert!(plan(&[odd]).play.is_none());
}

// ---- live: the four page states against Drift Rebels on the throwaway server ----
fn signed(base: &str) -> Client {
    let d = DeviceInfo { name: "show-test".into(), id: "show-test-device".into(), version: "0".into() };
    let (mut c, _) = Client::connect(base, d).unwrap();
    c.sign_in("tester", "test-pass-1").unwrap();
    c
}

#[test]
fn live_four_states() {
    let Ok(u) = std::env::var("JF_TEST_URL") else { return };
    let c = signed(&u);
    let ser = c.search("Drift Rebels").unwrap().into_iter().find(|i| i.kind == ItemKind::Series).unwrap();
    let eps = |c: &Client| c.episodes(&ser.id, None).unwrap();
    let set = |s: i32, e: i32, played: bool| {
        let id =
            eps(&c).into_iter().find(|x| x.parent_index_number == Some(s) && x.index_number == Some(e)).unwrap().id;
        c.set_played(&id, played).unwrap();
    };
    let all = [(1, 1), (1, 2), (2, 1), (2, 2), (3, 1), (3, 2)];
    for &(s, e) in &all {
        set(s, e, false);
    }
    assert_eq!((plan(&eps(&c)).open_season, label(&plan(&eps(&c)))), (1, "Play S1 E1".into()), "new show");
    set(1, 1, true);
    set(1, 2, true);
    set(2, 1, true);
    assert_eq!((plan(&eps(&c)).open_season, label(&plan(&eps(&c)))), (2, "Play S2 E2".into()), "mid-season");
    set(2, 2, true);
    assert_eq!((plan(&eps(&c)).open_season, label(&plan(&eps(&c)))), (3, "Play S3 E1".into()), "new season out");
    set(3, 1, true);
    set(3, 2, true);
    assert_eq!((plan(&eps(&c)).open_season, label(&plan(&eps(&c)))), (3, "Play S1 E1".into()), "all watched");
    for &(s, e) in &all {
        set(s, e, false);
    }
}
