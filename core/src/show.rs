//! F1 show page plan (SPEC R-F1-1/2): which season opens and what the Play button says, from one episode list.
//! Pure. Body: packet F1.
use crate::api::Item;

#[derive(Debug, Clone, PartialEq)]
pub struct PlayPick {
    pub episode_id: String,
    /// "Play S1 E1" · "Resume S2 E4 · 12 min left" · "Play S3 E1"
    pub label: String,
    pub resume: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ShowPlan {
    /// Season number to open (never 0 unless the show has only specials).
    pub open_season: i32,
    /// Seasons for the drop-down: regular seasons ascending, then 0 (Specials) if present.
    pub seasons: Vec<i32>,
    pub play: Option<PlayPick>,
}

/// `episodes`: every episode of the series (any order) with `UserData`, `RunTimeTicks`,
/// `ParentIndexNumber` (season) and `IndexNumber`. Episodes without a season or episode number are ignored.
pub fn plan(episodes: &[Item]) -> ShowPlan {
    let _ = episodes;
    todo!("F1")
}
