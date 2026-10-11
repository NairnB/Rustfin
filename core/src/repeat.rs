//! Player repeat mode (P-PLAY-15): what to do when an item reaches its end. Pure, so it is unit-tested.

/// The Repeat menu: Off / One / All. Kept for the session only (not in settings).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Repeat {
    #[default]
    Off,
    One,
    All,
}

impl Repeat {
    pub const ALL: [Repeat; 3] = [Repeat::Off, Repeat::One, Repeat::All];

    pub fn label(self) -> &'static str {
        match self {
            Repeat::Off => "Off",
            Repeat::One => "One",
            Repeat::All => "All",
        }
    }
}

/// What the player does at the end of an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndAction {
    /// Leave the player.
    Close,
    /// Show "Up next" for the next item.
    UpNext,
    /// Play the current item again from the start (no "Up next").
    Replay,
    /// Past the last item: start again at the first (a queue), or replay a single film or last episode.
    Restart,
}

/// `has_next`: there is a next item to go to (next episode or next in the queue).
pub fn end_action(mode: Repeat, has_next: bool) -> EndAction {
    match (mode, has_next) {
        (Repeat::One, _) => EndAction::Replay,
        (_, true) => EndAction::UpNext,
        (Repeat::All, false) => EndAction::Restart,
        (Repeat::Off, false) => EndAction::Close,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_follows_today() {
        assert_eq!(end_action(Repeat::Off, true), EndAction::UpNext); // episode with next
        assert_eq!(end_action(Repeat::Off, false), EndAction::Close); // film / last in queue
    }

    #[test]
    fn one_replays_everywhere() {
        assert_eq!(end_action(Repeat::One, true), EndAction::Replay);
        assert_eq!(end_action(Repeat::One, false), EndAction::Replay);
    }

    #[test]
    fn all_continues_then_restarts() {
        assert_eq!(end_action(Repeat::All, true), EndAction::UpNext);
        assert_eq!(end_action(Repeat::All, false), EndAction::Restart);
    }
}
