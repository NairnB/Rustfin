//! C3 rules: focus movement (R-NAV-1/2), scrolling, image cache (R-IMG-*). Lead's tests; builders never edit.
use jf_core::images::*;
use jf_core::nav::*;
use jf_core::platform::Key::*;

fn row(n: usize) -> Group {
    Group { len: n, cols: n, xs: (0..n).map(|i| 100.0 + i as f32 * 200.0).collect() }
}
fn grid(n: usize, cols: usize) -> Group {
    Group { len: n, cols, xs: (0..n).map(|i| 100.0 + (i % cols) as f32 * 200.0).collect() }
}
fn f(group: usize, index: usize) -> Focus {
    Focus { group, index }
}

#[test]
fn left_right_in_a_row_without_wrap() {
    let g = [row(4)];
    assert_eq!(move_focus(&g, f(0, 0), Right), f(0, 1));
    assert_eq!(move_focus(&g, f(0, 3), Right), f(0, 3));
    assert_eq!(move_focus(&g, f(0, 0), Left), f(0, 0));
    assert_eq!(move_focus(&g, f(0, 2), Left), f(0, 1));
    assert_eq!(move_focus(&g, f(0, 2), Enter), f(0, 2));
}

#[test]
fn grid_moves_and_row_edges() {
    let g = [grid(10, 4)]; // rows: 0-3, 4-7, 8-9
    assert_eq!(move_focus(&g, f(0, 1), Down), f(0, 5));
    assert_eq!(move_focus(&g, f(0, 5), Up), f(0, 1));
    assert_eq!(move_focus(&g, f(0, 3), Right), f(0, 3), "no wrap to next row");
    assert_eq!(move_focus(&g, f(0, 4), Left), f(0, 4));
    assert_eq!(move_focus(&g, f(0, 9), Right), f(0, 9));
    assert_eq!(move_focus(&g, f(0, 7), Down), f(0, 9), "above a short last row → last item");
    assert_eq!(move_focus(&g, f(0, 5), Down), f(0, 9));
    assert_eq!(move_focus(&g, f(0, 4), Down), f(0, 8));
    assert_eq!(move_focus(&g, f(0, 9), Down), f(0, 9), "nothing below");
    assert_eq!(move_focus(&g, f(0, 2), Up), f(0, 2), "nothing above");
}

#[test]
fn between_groups_keeps_nearest_x_and_skips_empty() {
    let bar = Group { len: 3, cols: 3, xs: vec![100.0, 300.0, 1700.0] };
    let empty = Group { len: 0, cols: 1, xs: vec![] };
    let g = [bar, empty, row(8), grid(10, 4)];
    assert_eq!(move_focus(&g, f(0, 2), Down), f(2, 7), "x 1700 → nearest in row is index 7 (x 1500)");
    assert_eq!(move_focus(&g, f(0, 1), Down), f(2, 1));
    assert_eq!(move_focus(&g, f(2, 1), Up), f(0, 1), "empty group skipped going up");
    assert_eq!(move_focus(&g, f(2, 6), Down), f(3, 3), "into a grid: first row, nearest x");
    assert_eq!(move_focus(&g, f(3, 1), Up), f(2, 6), "UXP1: the row remembers its last focus (6), not nearest x");
    assert_eq!(move_focus(&g, f(3, 9), Down), f(3, 9));
}

#[test]
fn up_into_grid_lands_on_its_last_row() {
    let g = [grid(10, 4), row(2)];
    assert_eq!(move_focus(&g, f(1, 0), Up), f(0, 8));
    assert_eq!(move_focus(&g, f(1, 1), Up), f(0, 9));
}

#[test]
fn invalid_focus_is_clamped_first() {
    let g = [row(3)];
    assert_eq!(move_focus(&g, f(0, 9), Left), f(0, 1));
    assert_eq!(move_focus(&g, f(5, 0), Enter), f(0, 0));
}

#[test]
fn columns_that_fit() {
    assert_eq!(grid_cols(1920.0, 64.0, 180.0, 24.0), 8);
    assert_eq!(grid_cols(390.0, 16.0, 180.0, 24.0), 1);
    assert_eq!(grid_cols(100.0, 16.0, 180.0, 24.0), 1);
}

#[test]
fn scroll_moves_minimally() {
    assert_eq!(scroll_to_show(0.0, 1000.0, 5000.0, 100.0, 400.0, 20.0), 0.0, "already visible");
    assert_eq!(scroll_to_show(0.0, 1000.0, 5000.0, 1200.0, 1500.0, 20.0), 520.0, "bottom + pad aligned");
    assert_eq!(scroll_to_show(2000.0, 1000.0, 5000.0, 1500.0, 1800.0, 20.0), 1480.0, "top - pad aligned");
    assert_eq!(scroll_to_show(0.0, 1000.0, 1200.0, 1100.0, 1190.0, 20.0), 200.0, "clamped to content end");
    assert_eq!(scroll_to_show(500.0, 1000.0, 5000.0, 5.0, 100.0, 20.0), 0.0, "clamped to 0");
}

#[test]
fn cache_key_ignores_api_key() {
    let a = cache_key("http://h/Items/1/Images/Primary?maxWidth=200&tag=x");
    assert_eq!(a.len(), 16);
    assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    assert_eq!(a, cache_key("http://h/Items/1/Images/Primary?maxWidth=200&tag=x&api_key=SECRET"));
    assert_eq!(a, cache_key("http://h/Items/1/Images/Primary?api_key=SECRET&maxWidth=200&tag=x"));
    assert_ne!(a, cache_key("http://h/Items/1/Images/Primary?maxWidth=240&tag=x"));
    // FNV-1a 64 of the empty string
    assert_eq!(cache_key(""), "cbf29ce484222325");
}

#[test]
fn disk_cache_put_get_and_trim() {
    let d = std::env::temp_dir().join(format!("jf-imgcache-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    let c = DiskCache::new(d.clone(), 1000).unwrap();
    assert!(c.get("a").is_none());
    c.put("a", &[1u8; 300]).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    c.put("b", &[2u8; 300]).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    c.put("c", &[3u8; 300]).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    assert_eq!(c.get("a").unwrap().len(), 300, "hit refreshes a");
    std::thread::sleep(std::time::Duration::from_millis(20));
    c.put("d", &[4u8; 300]).unwrap(); // 1200 > 1000 → trim to ≤ 900: evicts b (oldest), keeps a, c, d
    assert!(c.total_bytes() <= 900);
    assert!(c.get("b").is_none());
    assert!(c.get("a").is_some() && c.get("c").is_some() && c.get("d").is_some());
    assert!(d.join("a.img").exists());
}

#[test]
fn prefetch_window() {
    assert_eq!(wanted_rows(0, 3, 50, true), 0..5);
    assert_eq!(wanted_rows(10, 13, 50, false), 9..14);
    assert_eq!(wanted_rows(47, 49, 50, true), 47..50);
    assert_eq!(wanted_rows(0, 3, 50, false), 0..4);
    assert_eq!(wanted_rows(0, 0, 0, true), 0..0);
}

#[test]
fn lru_evicts_least_recent() {
    let mut l: Lru<u32, &str> = Lru::new(2);
    assert!(l.insert(1, "a").is_empty());
    assert!(l.insert(2, "b").is_empty());
    assert_eq!(l.get(&1), Some(&"a"));
    assert_eq!(l.insert(3, "c"), vec!["b"]);
    assert!(l.contains(&1) && l.contains(&3) && !l.contains(&2));
    assert_eq!(l.insert(3, "c2"), vec!["c"], "replacing hands back the old value");
    assert_eq!(l.len(), 2);
}
