//! F2 "Move to section…" and F4 "Position…" admin controls on the item page (R-F2-3, R-F4-2, R-TAG-6).
//! Draw nothing unless the signed-in user is a server admin; the item page calls these and forwards the result.
use crate::api::item_edit::identify_kind;
use crate::api::{ApiError, Item, ItemKind, Result};
use crate::app::Cx;
use crate::sections::section_of;
use crate::tags::{with_order, with_section, with_story, Era, RfTags, StoryYear};

/// Result tag the item page routes back through `SectionMove::on_done`.
pub const TAG: &str = "section";
/// Result tag for the F4 "Position…" write (R-F4-2).
pub const ORDER_TAG: &str = "order";
/// Result tag for the F3 "Story year…" write (R-F3-3).
pub const STORY_TAG: &str = "story";

/// Menu choices in order. The first is "Automatic" and writes no tag (R-F2-3).
pub const CHOICES: [(&str, Option<&str>); 6] = [
    ("Automatic", None),
    ("Movies", Some("Movies")),
    ("TV Shows", Some("TV Shows")),
    ("Documentaries", Some("Documentaries")),
    ("Specials", Some("Specials")),
    ("Other", Some("Other")),
];

/// The button label on the item page, e.g. "Section: Movies (move)".
pub fn button_label(item: &Item) -> String {
    format!("Section: {} (move)", section_of(item).label())
}

/// Spawn the tag write: choice 0 removes the tag, any other writes `rf-section:<name>` (R-TAG-6).
pub fn move_to(cx: &Cx, item_id: String, choice: usize) {
    let client = cx.api();
    let name: Option<String> = CHOICES.get(choice).and_then(|(_, n)| n.map(str::to_string));
    cx.spawn(TAG, move || client.update_tags(&item_id, |tags| with_section(tags, name.as_deref())));
}

/// State the item page keeps for the control.
#[derive(Default)]
pub struct SectionMove {
    pub open: bool,
    pub error: Option<String>,
}

impl SectionMove {
    pub fn new() -> SectionMove {
        SectionMove::default()
    }

    /// Handle the result of a `move_to` spawn. Returns `true` when the move succeeded and the item
    /// should be refreshed (so the new section shows).
    pub fn on_done(&mut self, cx: &mut Cx, payload: Box<dyn std::any::Any + Send>) -> bool {
        self.error = None;
        let Ok(r) = payload.downcast::<Result<Vec<String>>>() else { return false };
        match *r {
            Ok(_) => true,
            Err(ApiError::Unauthorized) => {
                cx.signed_out();
                false
            }
            Err(e) => {
                self.error = Some(e.to_string());
                false
            }
        }
    }
}

/// The button label for the F4 "Position…" control on the item page: "Position: 30 (move)" when set,
/// "Position… (move)" otherwise.
pub fn position_label(item: &Item, place: &str) -> String {
    match RfTags::parse(&item.tags).order.get(place) {
        Some(n) => format!("Position: {n} (move)"),
        None => "Position… (move)".to_string(),
    }
}

/// Spawn one `rf-order:<place>:<n>` write (R-F4-2, R-TAG-6).
pub fn set_position(cx: &Cx, item_id: String, place: String, n: u32) {
    let client = cx.api();
    cx.spawn(ORDER_TAG, move || client.update_tags(&item_id, |tags| with_order(tags, &place, Some(n))));
}

/// State the item page keeps for the F4 "Position…" number box.
#[derive(Default)]
pub struct PositionMove {
    pub open: bool,
    pub value: i64,
    pub error: Option<String>,
}

impl PositionMove {
    pub fn new() -> PositionMove {
        PositionMove::default()
    }

    /// Handle the result of a `set_position` spawn. Returns `true` when the write succeeded and the item
    /// should be refreshed (so the new position shows).
    pub fn on_done(&mut self, cx: &mut Cx, payload: Box<dyn std::any::Any + Send>) -> bool {
        self.error = None;
        let Ok(r) = payload.downcast::<Result<Vec<String>>>() else { return false };
        match *r {
            Ok(_) => true,
            Err(ApiError::Unauthorized) => {
                cx.signed_out();
                false
            }
            Err(e) => {
                self.error = Some(e.to_string());
                false
            }
        }
    }
}

/// The button label for the F3 "Story year…" control: "Story year: 19 BBY (edit)" when set, else
/// "Story year… (edit)".
pub fn story_label(item: &Item) -> String {
    match RfTags::parse(&item.tags).story {
        Some(s) => format!("Story year: {} (edit)", s.label()),
        None => "Story year… (edit)".to_string(),
    }
}

/// The editor's starting value: the item's story year, or 0 BBY.
pub fn story_value(item: &Item) -> (i64, Era) {
    match RfTags::parse(&item.tags).story {
        Some(s) => (i64::from(s.n), s.era),
        None => (0, Era::Bby),
    }
}

/// Spawn one `rf-story:` write; `None` clears it (R-F3-3, R-TAG-6).
pub fn set_story(cx: &Cx, item_id: String, story: Option<StoryYear>) {
    let client = cx.api();
    cx.spawn(STORY_TAG, move || client.update_tags(&item_id, |tags| with_story(tags, story)));
}

/// State the item page keeps for the F3 "Story year…" editor (number + BBY/ABY).
pub struct StoryEdit {
    pub open: bool,
    pub value: i64,
    pub era: Era,
    pub error: Option<String>,
}

impl Default for StoryEdit {
    fn default() -> Self {
        StoryEdit { open: false, value: 0, era: Era::Bby, error: None }
    }
}

impl StoryEdit {
    pub fn new() -> StoryEdit {
        StoryEdit::default()
    }

    /// The year the editor would save.
    pub fn year(&self) -> StoryYear {
        StoryYear { n: self.value.clamp(0, 100_000) as u32, era: self.era }
    }

    /// Handle the result of a `set_story` spawn. Returns `true` when the write succeeded and the item
    /// should be refreshed.
    pub fn on_done(&mut self, cx: &mut Cx, payload: Box<dyn std::any::Any + Send>) -> bool {
        self.error = None;
        let Ok(r) = payload.downcast::<Result<Vec<String>>>() else { return false };
        match *r {
            Ok(_) => true,
            Err(ApiError::Unauthorized) => {
                cx.signed_out();
                false
            }
            Err(e) => {
                self.error = Some(e.to_string());
                false
            }
        }
    }
}

// ---------- A6: the "More…" menu (P-ITEM-20, P-ADM-27, P-ITEM-12) ----------

/// What a "More…" row does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MoreAction {
    Edit,
    Identify,
    Refresh,
    Merge,
    Split,
    CopyLink,
    Delete,
}

/// The rows of the "More…" menu. A non-admin only gets "Copy link"; Identify is for films and shows, Merge for
/// films, Split only when the item has more than one version.
pub fn more_entries(kind: &ItemKind, is_admin: bool, sources: usize) -> Vec<(&'static str, MoreAction)> {
    let mut v = vec![];
    if is_admin {
        v.push(("Edit metadata", MoreAction::Edit));
        if matches!(kind, ItemKind::Movie | ItemKind::Series) {
            v.push(("Identify", MoreAction::Identify));
        }
        v.push(("Refresh metadata", MoreAction::Refresh));
        if *kind == ItemKind::Movie {
            v.push(("Merge versions", MoreAction::Merge));
            if sources > 1 {
                v.push(("Split versions", MoreAction::Split));
            }
        }
    }
    v.push(("Copy link", MoreAction::CopyLink));
    if is_admin {
        v.push(("Delete", MoreAction::Delete));
    }
    v
}

/// The kind name Identify searches with (`None` = not identifiable).
pub fn identify_for(kind: &ItemKind) -> Option<&'static str> {
    identify_kind(match kind {
        ItemKind::Movie => "Movie",
        ItemKind::Series => "Series",
        _ => "",
    })
}

/// The "More…" screen for an item (pushed from the item page).
pub fn more_screen(item: Item, is_admin: bool) -> Box<dyn crate::app::Screen> {
    super::admin::item_more::screen(item, is_admin)
}
