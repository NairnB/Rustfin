//! F2 sections inside a library (SPEC R-F2-1/2). Pure. Body: packet F2.
use crate::api::{Item, ItemKind};
use crate::tags::RfTags;

/// In page order (R-F2-1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Section {
    Movies,
    TvShows,
    Documentaries,
    Specials,
    Other,
}

impl Section {
    pub const ALL: [Section; 5] =
        [Section::Movies, Section::TvShows, Section::Documentaries, Section::Specials, Section::Other];
    /// "Movies", "TV Shows", "Documentaries", "Specials", "Other" — also the `rf-section:` value written.
    pub fn label(self) -> &'static str {
        match self {
            Section::Movies => "Movies",
            Section::TvShows => "TV Shows",
            Section::Documentaries => "Documentaries",
            Section::Specials => "Specials",
            Section::Other => "Other",
        }
    }
    /// Exact label (case-insensitive) → section; anything else → Other.
    pub fn from_name(name: &str) -> Section {
        match name.trim().to_ascii_lowercase().as_str() {
            "movies" => Section::Movies,
            "tv shows" => Section::TvShows,
            "documentaries" => Section::Documentaries,
            "specials" => Section::Specials,
            _ => Section::Other,
        }
    }
}

/// R-F2-2: `rf-section:` tag wins; else genre Documentary (case-insensitive) → Documentaries; Movie → Movies;
/// Series → TvShows; anything else → Other.
pub fn section_of(item: &Item) -> Section {
    if let Some(name) = RfTags::parse(&item.tags).section {
        return Section::from_name(&name);
    }
    if item.genres.iter().any(|g| g.eq_ignore_ascii_case("Documentary")) {
        return Section::Documentaries;
    }
    match item.kind {
        ItemKind::Movie => Section::Movies,
        ItemKind::Series => Section::TvShows,
        _ => Section::Other,
    }
}

/// Items grouped into sections in `Section::ALL` order; empty sections left out; items keep their input order.
pub fn group(items: &[Item]) -> Vec<(Section, Vec<Item>)> {
    let mut buckets: Vec<(Section, Vec<Item>)> = Section::ALL.iter().map(|&s| (s, Vec::new())).collect();
    for it in items {
        let s = section_of(it);
        if let Some(b) = buckets.iter_mut().find(|(sec, _)| *sec == s) {
            b.1.push(it.clone());
        }
    }
    buckets.retain(|(_, v)| !v.is_empty());
    buckets
}
