//! F1 show page plan (SPEC R-F1-1/2): which season opens and what the Play button says, from one episode list.
//! Pure. Body: packet F1.
use crate::api::{ticks_to_secs, Item};

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
    let mut numbered: Vec<&Item> =
        episodes.iter().filter(|e| e.parent_index_number.is_some() && e.index_number.is_some()).collect();
    // Series order: season, then episode. Specials (season 0) sort before regular seasons.
    numbered.sort_by_key(|e| (e.parent_index_number.unwrap(), e.index_number.unwrap()));

    // Drop-down seasons: regular seasons ascending, then 0 (Specials) if present.
    let mut seasons: Vec<i32> = vec![];
    let mut has_specials = false;
    for e in &numbered {
        let s = e.parent_index_number.unwrap();
        if s == 0 {
            has_specials = true;
        } else if !seasons.contains(&s) {
            seasons.push(s);
        }
    }
    if has_specials {
        seasons.push(0);
    }

    let played = |e: &Item| e.user_data.as_ref().map(|u| u.played).unwrap_or(false);
    let pos = |e: &Item| e.user_data.as_ref().map(|u| u.playback_position_ticks).unwrap_or(0);

    // Specials are never chosen by default: the play pick only looks at regular episodes.
    let regular: Vec<&Item> = numbered.iter().copied().filter(|e| e.parent_index_number.unwrap() > 0).collect();

    if regular.is_empty() {
        return ShowPlan { open_season: 0, seasons, play: None };
    }

    // R-F1-2: an episode in progress (position > 0, not played), latest in series order, wins.
    let resume = regular.iter().copied().filter(|e| pos(e) > 0 && !played(e)).next_back();
    if let Some(e) = resume {
        let s = e.parent_index_number.unwrap();
        let ep = e.index_number.unwrap();
        let total = e.run_time_ticks.map(ticks_to_secs).unwrap_or(0.0);
        let remaining = (total - ticks_to_secs(pos(e))).max(0.0);
        let m = ((remaining / 60.0).ceil() as i64).max(1);
        return ShowPlan {
            open_season: s,
            seasons,
            play: Some(PlayPick {
                episode_id: e.id.clone(),
                label: format!("Resume S{s} E{ep} · {m} min left"),
                resume: true,
            }),
        };
    }

    let first = regular[0];
    let first_label = format!("Play S{} E{}", first.parent_index_number.unwrap(), first.index_number.unwrap());

    match regular.iter().rposition(|e| played(e)) {
        // Nothing played: Play S1 E1 of the first regular season.
        None => ShowPlan {
            open_season: first.parent_index_number.unwrap(),
            seasons,
            play: Some(PlayPick { episode_id: first.id.clone(), label: first_label, resume: false }),
        },
        Some(i) => match regular.iter().skip(i + 1).find(|e| !played(e)) {
            // First unplayed regular episode after the last played regular one.
            Some(next) => ShowPlan {
                open_season: next.parent_index_number.unwrap(),
                seasons,
                play: Some(PlayPick {
                    episode_id: next.id.clone(),
                    label: format!("Play S{} E{}", next.parent_index_number.unwrap(), next.index_number.unwrap()),
                    resume: false,
                }),
            },
            // Everything regular played: open the last regular season, button Play S1 E1.
            None => ShowPlan {
                open_season: regular.last().unwrap().parent_index_number.unwrap(),
                seasons,
                play: Some(PlayPick { episode_id: first.id.clone(), label: first_label, resume: false }),
            },
        },
    }
}
