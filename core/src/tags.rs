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
        match self.era {
            Era::Bby => -(self.n as i64),
            Era::Aby => self.n as i64,
        }
    }
    /// "0 BBY", "4 ABY".
    pub fn label(&self) -> String {
        let era = match self.era {
            Era::Bby => "BBY",
            Era::Aby => "ABY",
        };
        format!("{} {era}", self.n)
    }
    /// "0 BBY" / "4 aby" / " 32 BBY " → Some; anything else → None.
    pub fn parse(s: &str) -> Option<StoryYear> {
        let mut it = s.split_whitespace();
        let n: u32 = it.next()?.parse().ok()?;
        let era = match it.next()?.to_ascii_lowercase().as_str() {
            "bby" => Era::Bby,
            "aby" => Era::Aby,
            _ => return None,
        };
        if it.next().is_some() {
            return None;
        }
        Some(StoryYear { n, era })
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
        let mut out = RfTags::default();
        for t in tags {
            let Some(rest) = t.strip_prefix(PREFIX) else { continue };
            if let Some(name) = rest.strip_prefix("section:") {
                out.section = Some(name.trim().to_string());
            } else if let Some(s) = rest.strip_prefix("story:") {
                if let Some(y) = StoryYear::parse(s) {
                    out.story = Some(y);
                }
            } else if let Some(rest) = rest.strip_prefix("order:") {
                if let Some((place, n)) = rest.rsplit_once(':') {
                    let place = place.trim();
                    if let Ok(n) = n.trim().parse::<u32>() {
                        if !place.is_empty() {
                            out.order.insert(place.to_string(), n);
                        }
                    }
                }
            }
        }
        out
    }
}

/// R-TAG-2: remove every `rf-section:` tag, then append `rf-section:<name>` if Some.
pub fn with_section(tags: &[String], name: Option<&str>) -> Vec<String> {
    let mut out: Vec<String> = tags.iter().filter(|t| !t.starts_with("rf-section:")).cloned().collect();
    if let Some(name) = name {
        out.push(format!("rf-section:{name}"));
    }
    out
}

/// R-TAG-2: same for `rf-story:`.
pub fn with_story(tags: &[String], story: Option<StoryYear>) -> Vec<String> {
    let mut out: Vec<String> = tags.iter().filter(|t| !t.starts_with("rf-story:")).cloned().collect();
    if let Some(story) = story {
        out.push(format!("rf-story:{}", story.label()));
    }
    out
}

/// R-TAG-2: same for `rf-order:<place>:` (other places untouched).
pub fn with_order(tags: &[String], place: &str, n: Option<u32>) -> Vec<String> {
    let prefix = format!("rf-order:{place}:");
    let mut out: Vec<String> = tags.iter().filter(|t| !t.starts_with(&prefix)).cloned().collect();
    if let Some(n) = n {
        out.push(format!("rf-order:{place}:{n}"));
    }
    out
}

/// R-TAG-4: first 8 hex chars of a collection/library id (lower case, dashes removed).
pub fn place_id(collection_or_library_id: &str) -> String {
    collection_or_library_id.chars().filter(|c| c.is_ascii_hexdigit()).map(|c| c.to_ascii_lowercase()).take(8).collect()
}
