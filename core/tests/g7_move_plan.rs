//! G7: `move_plan` against a model of Jellyfin 10.10.7's Move call (measured on jf-test: an upward Move lands at
//! max(newIndex, 1), a downward Move lands at newIndex + 1 capped at the end, newIndex == position is a no-op).
use jf_core::api::move_plan;

fn server_move(list: &mut Vec<usize>, entry: usize, new_index: usize) {
    let n = list.len();
    let pos = list.iter().position(|&e| e == entry).unwrap();
    let landing = if new_index > pos {
        (new_index + 1).min(n - 1)
    } else if new_index < pos {
        new_index.max(1)
    } else {
        pos
    };
    let e = list.remove(pos);
    list.insert(landing, e);
}

#[test]
fn every_move_lands_where_asked() {
    for n in 2..=7usize {
        for from in 0..n {
            for to in 0..n {
                let mut list: Vec<usize> = (0..n).collect();
                let mut want = list.clone();
                let e = want.remove(from);
                want.insert(to, e);
                for (entry, new_index) in move_plan(n, from, to) {
                    server_move(&mut list, entry, new_index);
                }
                assert_eq!(list, want, "n={n} from={from} to={to}");
            }
        }
    }
}

#[test]
fn no_move_needs_no_calls() {
    assert!(move_plan(5, 2, 2).is_empty());
    assert!(move_plan(5, 9, 1).is_empty());
    assert!(move_plan(5, 1, 9).is_empty());
}

#[test]
fn simple_moves_are_one_call() {
    assert_eq!(move_plan(5, 3, 2).len(), 1);
    assert_eq!(move_plan(5, 1, 2).len(), 1);
    assert_eq!(move_plan(5, 0, 4).len(), 1);
}
