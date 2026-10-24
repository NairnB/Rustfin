//! Remote-style focus movement (SPEC §7, R-NAV-1/2). Pure logic, no egui. Bodies are packet C3.
use crate::platform::Key;
use std::cell::RefCell;
use std::collections::HashMap;

type Layout = Vec<(usize, usize)>;

thread_local! {
    /// Last focused index of each group, per screen layout (the `(len, cols)` of every group). Screens rebuild their
    /// groups every frame, so the memory lives here, keyed by the layout rather than by an id they would have to keep.
    static MEMORY: RefCell<HashMap<Layout, Vec<Option<usize>>>> = RefCell::new(HashMap::new());
    /// NAV: set when a Left could not move (focus at the screen's left edge); the desktop shell reads it to step into
    /// the sidebar.
    static LEFT_EDGE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// NAV: true if a Left reached the left edge since the last call (and clears it).
pub fn take_left_edge() -> bool {
    LEFT_EDGE.with(|e| e.replace(false))
}

fn layout(groups: &[Group]) -> Layout {
    groups.iter().map(|g| (g.len, g.cols)).collect()
}

fn remember(groups: &[Group], f: Focus) {
    MEMORY.with(|m| {
        let mut m = m.borrow_mut();
        let slots = m.entry(layout(groups)).or_insert_with(|| vec![None; groups.len()]);
        slots[f.group] = Some(f.index);
    });
}

/// Forget where focus last was in `group` for this layout, so the next entry lands on its first item (a season switch).
pub fn forget(groups: &[Group], group: usize) {
    MEMORY.with(|m| {
        if let Some(slot) = m.borrow_mut().get_mut(&layout(groups)).and_then(|s| s.get_mut(group)) {
            *slot = None;
        }
    });
}

fn remembered(groups: &[Group], group: usize) -> Option<usize> {
    MEMORY.with(|m| {
        let i = m.borrow().get(&layout(groups))?.get(group).copied().flatten()?;
        (i < groups[group].len).then_some(i)
    })
}

/// A `cols == 1` group whose next (`right`) or previous item sits to the side of `index` — a row of buttons stored as a list.
fn beside(g: &Group, index: usize, right: bool) -> bool {
    if g.cols > 1 || g.xs.len() != g.len {
        return false;
    }
    if right {
        index + 1 < g.len && g.xs[index + 1] > g.xs[index] + 1.0
    } else {
        index > 0 && g.xs[index - 1] < g.xs[index] - 1.0
    }
}

/// A group that is one row of two or more items.
fn is_row(g: &Group) -> bool {
    g.len > 1 && rows_of(g) == 1
}

/// Where focus lands when it enters group `gi` from `src` (item `src_index`) with no `remembered` index:
/// a lone control (field) above or below a row goes to the row's first item, otherwise the nearest x in `[start, end]`.
fn enter(groups: &[Group], gi: usize, src: &Group, src_index: usize, start: usize, end: usize) -> usize {
    if let Some(i) = remembered(groups, gi) {
        return i;
    }
    let tg = &groups[gi];
    if src.len == 1 && is_row(tg) {
        return start;
    }
    nearest(tg, start, end, x_of(src, src_index))
}

/// One focus group on a screen: a row (`cols == len`), a grid (`cols` per row) or a vertical list (`cols == 1`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Group {
    pub len: usize,
    pub cols: usize,
    /// On-screen x centre of each item (same length as `len`), used to keep the column when changing group.
    /// Empty → use the column number as x.
    pub xs: Vec<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Focus {
    pub group: usize,
    pub index: usize,
}

/// Moves focus for an arrow key; any other key returns `f` unchanged.
/// - Left/Right: within the current row of the group, no wrapping.
/// - Down: next row of the group (same column); from the row above a shorter last row → the last item;
///   from the last row → the next non-empty group, first row, item with the nearest x.
/// - Up: previous row; from the first row → the previous non-empty group, its **last** row, nearest x.
/// - No group in that direction → unchanged. Empty groups are skipped.
/// - Entering a group by Up/Down lands on the item last focused there (UXP1); with none remembered, a lone control
///   (field) goes to the first item of a row, else the nearest x. Memory is per screen layout, see `MEMORY`.
/// - A group with `cols == 1` whose items sit side by side (rising x) also moves with Left/Right.
/// - An invalid `f` (group out of range, index ≥ len) is first clamped to the nearest valid focus.
pub fn move_focus(groups: &[Group], f: Focus, key: Key) -> Focus {
    let Some(cur) = clamp(groups, f) else {
        return f;
    };
    let g = &groups[cur.group];
    let cols = cols_of(g);
    remember(groups, cur);
    match key {
        Key::Left => {
            if cur.index % cols > 0 || beside(g, cur.index, false) {
                Focus { index: cur.index - 1, ..cur }
            } else {
                LEFT_EDGE.with(|e| e.set(true));
                cur
            }
        }
        Key::Right => {
            if (cur.index % cols + 1 < cols && cur.index + 1 < g.len) || beside(g, cur.index, true) {
                Focus { index: cur.index + 1, ..cur }
            } else {
                cur
            }
        }
        Key::Down => {
            let row = cur.index / cols;
            if row + 1 < rows_of(g) {
                let next = cur.index + cols;
                Focus { index: if next >= g.len { g.len - 1 } else { next }, ..cur }
            } else {
                match next_group_down(groups, cur.group) {
                    Some(gi) => {
                        let tg = &groups[gi];
                        let end = cols_of(tg).min(tg.len) - 1;
                        Focus { group: gi, index: enter(groups, gi, g, cur.index, 0, end) }
                    }
                    None => cur,
                }
            }
        }
        Key::Up => {
            if cur.index / cols > 0 {
                Focus { index: cur.index - cols, ..cur }
            } else {
                match next_group_up(groups, cur.group) {
                    Some(gi) => {
                        let tg = &groups[gi];
                        let start = (rows_of(tg) - 1) * cols_of(tg);
                        Focus { group: gi, index: enter(groups, gi, g, cur.index, start, tg.len - 1) }
                    }
                    None => cur,
                }
            }
        }
        _ => cur,
    }
}

fn cols_of(g: &Group) -> usize {
    g.cols.max(1)
}

fn rows_of(g: &Group) -> usize {
    if g.len == 0 {
        0
    } else {
        g.len.div_ceil(cols_of(g))
    }
}

/// On-screen x of an item; empty `xs` → column number.
fn x_of(g: &Group, index: usize) -> f32 {
    g.xs.get(index).copied().unwrap_or((index % cols_of(g)) as f32)
}

/// Index in `[start, end]` whose x is nearest `src_x`.
fn nearest(g: &Group, start: usize, end: usize, src_x: f32) -> usize {
    (start..=end)
        .min_by(|&a, &b| {
            let da = (x_of(g, a) - src_x).abs();
            let db = (x_of(g, b) - src_x).abs();
            da.total_cmp(&db)
        })
        .unwrap()
}

/// Nearest non-empty group after `from`; `None` past the end.
fn next_group_down(groups: &[Group], from: usize) -> Option<usize> {
    groups.iter().enumerate().skip(from + 1).find(|(_, g)| g.len > 0).map(|(i, _)| i)
}

/// Nearest non-empty group before `from`; `None` past the start.
fn next_group_up(groups: &[Group], from: usize) -> Option<usize> {
    groups[..from].iter().enumerate().rev().find(|(_, g)| g.len > 0).map(|(i, _)| i)
}

/// Clamp an invalid focus to the nearest valid one (nearest non-empty group, index < len).
fn clamp(groups: &[Group], f: Focus) -> Option<Focus> {
    if groups.is_empty() {
        return None;
    }
    let gi = if f.group < groups.len() && groups[f.group].len > 0 {
        f.group
    } else {
        groups.iter().enumerate().filter(|(_, g)| g.len > 0).map(|(i, _)| i).min_by_key(|&i| i.abs_diff(f.group))?
    };
    Some(Focus { group: gi, index: f.index.min(groups[gi].len - 1) })
}

/// R-NAV-2: columns that fit: floor((width - 2*margin + gap) / (card_w + gap)), at least 1.
pub fn grid_cols(width: f32, margin: f32, card_w: f32, gap: f32) -> usize {
    let n = ((width - 2.0 * margin + gap) / (card_w + gap)).floor() as i64;
    n.max(1) as usize
}

/// Scroll offset (top of viewport, content coordinates) that keeps `[item_top, item_bottom]` fully visible with
/// `pad` around it, moving as little as possible from `current`. Never below 0 or above `content_h - view_h`.
pub fn scroll_to_show(current: f32, view_h: f32, content_h: f32, item_top: f32, item_bottom: f32, pad: f32) -> f32 {
    let lo = item_top - pad;
    let hi = item_bottom + pad;
    let target = if lo >= current && hi <= current + view_h {
        current
    } else if hi > current + view_h {
        hi - view_h
    } else {
        lo
    };
    let max = (content_h - view_h).max(0.0);
    target.clamp(0.0, max)
}
