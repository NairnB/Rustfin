//! F2 sections inside a library (SPEC R-F2-1/2). Pure. Body: packet F2.
use crate::api::Item;

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
        todo!("F2")
    }
    /// Exact label (case-insensitive) → section; anything else → Other.
    pub fn from_name(name: &str) -> Section {
        let _ = name;
        todo!("F2")
    }
}

/// R-F2-2: `rf-section:` tag wins; else genre Documentary (case-insensitive) → Documentaries; Movie → Movies;
/// Series → TvShows; anything else → Other.
pub fn section_of(item: &Item) -> Section {
    let _ = item;
    todo!("F2")
}

/// Items grouped into sections in `Section::ALL` order; empty sections left out; items keep their input order.
pub fn group(items: &[Item]) -> Vec<(Section, Vec<Item>)> {
    let _ = items;
    todo!("F2")
}
