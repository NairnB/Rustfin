//! Rustfin's own server tags (SPEC R-TAG). Pure: parsing and edits of an item's tag list. Bodies: packet T1.
use std::collections::BTreeMap;

pub const PREFIX: &str = "rf-";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Era {
    Bby,
    Aby,
}

/// R-TAG-3
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoryYear {
    pub n: u32,
    pub era: Era,
}

impl StoryYear {
    /// BBY → −n, ABY → +n.
    pub fn key(&self) -> i64 {
        todo!("T1")
    }
    /// "0 BBY", "4 ABY".
    pub fn label(&self) -> String {
        todo!("T1")
    }
    /// "0 BBY" / "4 aby" / " 32 BBY " → Some; anything else → None.
    pub fn parse(s: &str) -> Option<StoryYear> {
        let _ = s;
        todo!("T1")
    }
}

/// What Rustfin reads from one item's tags (R-TAG-1). Malformed `rf-` tags are ignored here.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RfTags {
    pub section: Option<String>,
    pub story: Option<StoryYear>,
    /// place id → position
    pub order: BTreeMap<String, u32>,
}

impl RfTags {
    pub fn parse(tags: &[String]) -> RfTags {
        let _ = tags;
        todo!("T1")
    }
}

/// R-TAG-2: remove every `rf-section:` tag, then append `rf-section:<name>` if Some.
pub fn with_section(tags: &[String], name: Option<&str>) -> Vec<String> {
    let _ = (tags, name);
    todo!("T1")
}

/// R-TAG-2: same for `rf-story:`.
pub fn with_story(tags: &[String], story: Option<StoryYear>) -> Vec<String> {
    let _ = (tags, story);
    todo!("T1")
}

/// R-TAG-2: same for `rf-order:<place>:` (other places untouched).
pub fn with_order(tags: &[String], place: &str, n: Option<u32>) -> Vec<String> {
    let _ = (tags, place, n);
    todo!("T1")
}

/// R-TAG-4: first 8 hex chars of a collection/library id (lower case, dashes removed).
pub fn place_id(collection_or_library_id: &str) -> String {
    let _ = collection_or_library_id;
    todo!("T1")
}
