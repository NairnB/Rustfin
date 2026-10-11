//! Play queue (P-PLAY-22): the now-playing list with a current item. Pure, so it is unit-tested.
//! Items before the current one are played (kept at the top, greyed); only items after it can be removed or moved.
use crate::repeat::Repeat;

#[derive(Debug, Clone, PartialEq)]
pub struct Queue<T> {
    items: Vec<T>,
    current: usize,
}

impl<T> Queue<T> {
    /// `current` is clamped into the list.
    pub fn new(items: Vec<T>, current: usize) -> Queue<T> {
        let current = current.min(items.len().saturating_sub(1));
        Queue { items, current }
    }

    pub fn items(&self) -> &[T] {
        &self.items
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn current_index(&self) -> usize {
        self.current
    }

    pub fn current(&self) -> Option<&T> {
        self.items.get(self.current)
    }

    pub fn get(&self, i: usize) -> Option<&T> {
        self.items.get(i)
    }

    pub fn is_played(&self, i: usize) -> bool {
        i < self.current
    }

    pub fn is_upcoming(&self, i: usize) -> bool {
        i > self.current && i < self.items.len()
    }

    /// Make item `i` the current one. `false` (nothing changes) when `i` is out of range.
    pub fn jump(&mut self, i: usize) -> bool {
        if i >= self.items.len() {
            return false;
        }
        self.current = i;
        true
    }

    /// Drop upcoming item `i`. The current and played items stay.
    pub fn remove(&mut self, i: usize) -> bool {
        if !self.is_upcoming(i) {
            return false;
        }
        self.items.remove(i);
        true
    }

    pub fn can_move_up(&self, i: usize) -> bool {
        self.is_upcoming(i) && i - 1 > self.current
    }

    pub fn can_move_down(&self, i: usize) -> bool {
        self.is_upcoming(i) && i + 1 < self.items.len()
    }

    /// Move upcoming item `i` one place towards the current item; it never passes it.
    pub fn move_up(&mut self, i: usize) -> bool {
        if !self.can_move_up(i) {
            return false;
        }
        self.items.swap(i, i - 1);
        true
    }

    /// Move upcoming item `i` one place towards the end.
    pub fn move_down(&mut self, i: usize) -> bool {
        if !self.can_move_down(i) {
            return false;
        }
        self.items.swap(i, i + 1);
        true
    }

    /// Index to play when the current item ends: One repeats it, All wraps to the first, Off stops at the end.
    pub fn next(&self, repeat: Repeat) -> Option<usize> {
        if self.items.is_empty() {
            return None;
        }
        match repeat {
            Repeat::One => Some(self.current),
            _ if self.current + 1 < self.items.len() => Some(self.current + 1),
            Repeat::All => Some(0),
            Repeat::Off => None,
        }
    }

    /// Index of the item before the current one.
    pub fn previous(&self) -> Option<usize> {
        self.current.checked_sub(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(n: usize, current: usize) -> Queue<usize> {
        Queue::new((0..n).collect(), current)
    }

    #[test]
    fn current_is_clamped() {
        assert_eq!(q(3, 9).current_index(), 2);
        assert_eq!(q(0, 0).current(), None);
    }

    #[test]
    fn played_and_upcoming() {
        let q = q(5, 2);
        assert!(q.is_played(0) && q.is_played(1) && !q.is_played(2));
        assert!(!q.is_upcoming(2) && q.is_upcoming(3) && q.is_upcoming(4) && !q.is_upcoming(5));
    }

    #[test]
    fn jump_sets_current_and_rejects_out_of_range() {
        let mut q = q(5, 0);
        assert!(q.jump(2));
        assert_eq!(q.current(), Some(&2));
        assert!(q.jump(0)); // back to a played item
        assert!(!q.jump(5));
        assert_eq!(q.current_index(), 0);
    }

    #[test]
    fn remove_only_upcoming() {
        let mut q = q(5, 2);
        assert!(!q.remove(1) && !q.remove(2) && !q.remove(9));
        assert!(q.remove(3));
        assert_eq!(q.items(), &[0, 1, 2, 4]);
        assert_eq!(q.current(), Some(&2));
    }

    #[test]
    fn move_stays_among_upcoming() {
        let mut q = q(6, 1);
        assert!(!q.move_up(2)); // would pass the current item
        assert!(q.move_up(4));
        assert_eq!(q.items(), &[0, 1, 2, 4, 3, 5]);
        assert!(q.move_down(3));
        assert_eq!(q.items(), &[0, 1, 2, 3, 4, 5]);
        assert!(!q.move_down(5)); // last
        assert!(!q.move_up(0) && !q.move_down(1));
        assert_eq!(q.current(), Some(&1));
    }

    #[test]
    fn next_follows_repeat_rules() {
        let mut q = q(3, 0);
        assert_eq!(q.next(Repeat::Off), Some(1));
        assert_eq!(q.next(Repeat::One), Some(0));
        q.jump(2);
        assert_eq!(q.next(Repeat::Off), None);
        assert_eq!(q.next(Repeat::All), Some(0));
        assert_eq!(q.next(Repeat::One), Some(2));
        assert_eq!(Queue::<usize>::new(vec![], 0).next(Repeat::All), None);
    }

    #[test]
    fn previous_is_the_item_above() {
        let mut q = q(3, 0);
        assert_eq!(q.previous(), None);
        q.jump(2);
        assert_eq!(q.previous(), Some(1));
    }
}
