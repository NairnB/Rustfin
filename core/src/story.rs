//! F3 story calendar (SPEC R-F3-1/2). Pure. Bodies: packet F3.
use crate::api::Item;
use crate::tags::RfTags;
use std::cmp::Ordering;

/// True when at least one item has a valid `rf-story:` tag (then "Story order" is offered).
pub fn any_story(items: &[Item]) -> bool {
    items.iter().any(|it| RfTags::parse(&it.tags).story.is_some())
}

/// Items with a story year by `StoryYear::key()` (ties by name), then the rest A–Z by name.
pub fn sort_story(items: &mut [Item]) {
    let key = |it: &Item| RfTags::parse(&it.tags).story.map(|s| s.key());
    items.sort_by(|a, b| match (key(a), key(b)) {
        (Some(ak), Some(bk)) => ak.cmp(&bk).then_with(|| name_cmp(a, b)),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => name_cmp(a, b),
    });
}

fn name_cmp(a: &Item, b: &Item) -> Ordering {
    a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.name.cmp(&b.name))
}

/// The year line for cards and item pages: "1977 · 0 BBY", "1977", "0 BBY" or "" depending on what the item has and
/// on the two Settings switches.
pub fn year_line(item: &Item, show_release: bool, show_story: bool) -> String {
    let release = if show_release { item.production_year.map(|y| y.to_string()) } else { None };
    let story = if show_story { RfTags::parse(&item.tags).story.map(|s| s.label()) } else { None };
    [release, story].into_iter().flatten().collect::<Vec<_>>().join(" · ")
}
