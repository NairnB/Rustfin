//! Key-down state for the TV shells.
//!
//! Both TV shells send key-up events (EFL `EcoreEventType.KeyUp`, DOM `keyup`): then a key is down from its
//! key-down until its key-up, so a tap is up the instant it is released (no added delay) and a hold is simply
//! "still down at 0.6 s". `KeyHold::with_key_up()` is that mode. A stale key (no event for `STALE` seconds,
//! e.g. a lost key-up) is released anyway.
//!
//! A shell that really has no key-up falls back to repeat timing (`KeyHold::default()`): a lone press counts as
//! down for at most `FIRST_GAP` (kept below the 0.6 s hold so a tap is never read as a hold), and after a repeat
//! for `REPEAT_GAP` since the last one. Seeing a first real key-up switches to the exact mode for good.
use crate::platform::Key;
use std::collections::{HashMap, HashSet};

const FIRST_GAP: f64 = 0.58;
const REPEAT_GAP: f64 = 0.3;
const STALE: f64 = 5.0;

struct St {
    first: f64,
    last: f64,
    repeated: bool,
}

#[derive(Default)]
pub struct KeyHold {
    keys: HashMap<Key, St>,
    key_up: bool,
}

impl KeyHold {
    /// The shell delivers key-up events.
    pub fn with_key_up() -> KeyHold {
        KeyHold { keys: HashMap::new(), key_up: true }
    }

    fn is_down(&self, s: &St, now: f64) -> bool {
        if self.key_up {
            now - s.last < STALE
        } else {
            now - s.last < if s.repeated { REPEAT_GAP } else { FIRST_GAP }
        }
    }

    /// A key-down event (first press or auto-repeat) at `now` seconds.
    pub fn press(&mut self, k: Key, now: f64) {
        let continuing = self.keys.get(&k).is_some_and(|s| self.is_down(s, now));
        if continuing {
            if let Some(s) = self.keys.get_mut(&k) {
                s.last = now;
                s.repeated = true;
            }
        } else {
            self.keys.insert(k, St { first: now, last: now, repeated: false });
        }
    }

    /// A key-up event. The shell has key-up events, so the exact mode applies from now on.
    pub fn release(&mut self, k: Key) {
        self.key_up = true;
        self.keys.remove(&k);
    }

    pub fn down(&self, now: f64) -> HashSet<Key> {
        self.keys.iter().filter(|(_, s)| self.is_down(s, now)).map(|(k, _)| *k).collect()
    }

    pub fn held_secs(&self, k: Key, now: f64) -> Option<f64> {
        self.keys.get(&k).filter(|s| self.is_down(s, now)).map(|s| now - s.first)
    }
}
