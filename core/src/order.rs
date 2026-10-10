//! F4 "My order" (SPEC R-F4-1). Pure. Bodies: packet F4.
use crate::api::Item;
use crate::tags::RfTags;
use std::cmp::Ordering;

/// Items with an `rf-order:<place>:<n>` tag by n (ties by name), then the rest A–Z by name (case-insensitive).
pub fn sort_my_order(items: &mut [Item], place: &str) {
    let pos = |it: &Item| RfTags::parse(&it.tags).order.get(place).copied();
    items.sort_by(|a, b| match (pos(a), pos(b)) {
        (Some(an), Some(bn)) => an.cmp(&bn).then_with(|| name_cmp(a, b)),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => name_cmp(a, b),
    });
}

fn name_cmp(a: &Item, b: &Item) -> Ordering {
    a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.name.cmp(&b.name))
}

/// Tag writes `(item id, new position)` for moving the item at index `from` to index `to` of `shown` (a list already
/// in My order). Rules:
/// - Let the new order be `shown` with the item moved. The moved item lands at index k.
/// - If every item before k has a position: prev = position of item k-1 (0 if k = 0); next = position of item k+1 if
///   it has one, else prev + 20. If next - prev >= 2 → exactly one write: (moved, prev + (next - prev) / 2).
/// - Otherwise (no gap, or unpositioned items before k): renumber items 0..=k of the new order to 10, 20, 30, …
///   and return only the writes whose value changes (in list order).
/// - from == to → no writes.
pub fn move_writes(shown: &[Item], place: &str, from: usize, to: usize) -> Vec<(String, u32)> {
    if from == to || from >= shown.len() || to >= shown.len() {
        return vec![];
    }
    let mut new_order: Vec<Item> = shown.to_vec();
    let moved = new_order.remove(from);
    new_order.insert(to, moved);
    let k = to;
    let pos = |it: &Item| RfTags::parse(&it.tags).order.get(place).copied();

    // One write when every item before k is positioned and the gap either side of k is at least 2.
    if new_order[..k].iter().all(|it| pos(it).is_some()) {
        let prev = if k == 0 { 0 } else { pos(&new_order[k - 1]).unwrap_or(0) };
        let next = new_order.get(k + 1).and_then(&pos).unwrap_or(prev + 20);
        if next >= prev + 2 {
            return vec![(new_order[k].id.clone(), prev + (next - prev) / 2)];
        }
    }

    // No gap: renumber items 0..=k to 10, 20, 30, … and keep only the writes whose value changes.
    let mut writes = vec![];
    for (i, it) in new_order.iter().enumerate().take(k + 1) {
        let new_val = (i as u32 + 1) * 10;
        if pos(it) != Some(new_val) {
            writes.push((it.id.clone(), new_val));
        }
    }
    writes
}
