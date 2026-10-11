//! RR4 / P-PLAY-21: trickplay (seek-bar thumbnail) info for an item and the position → tile-sheet maths.
use serde::Deserialize;
use std::collections::BTreeMap;

/// One trickplay resolution of an item, as the server lists it under `Trickplay.<mediaSourceId>.<width>`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct TrickplayInfo {
    pub width: u32,
    pub height: u32,
    pub tile_width: u32,
    pub tile_height: u32,
    pub thumbnail_count: u32,
    /// Milliseconds between two thumbnails.
    pub interval: u64,
}

/// Where a thumbnail sits: tile sheet number, and the cell's top-left corner in pixels inside that sheet.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrickplayCell {
    pub sheet: u32,
    pub x: u32,
    pub y: u32,
}

impl TrickplayInfo {
    /// The thumbnail for `pos` seconds (clamped to the first and last one). None when the info is unusable.
    pub fn locate(&self, pos: f64) -> Option<TrickplayCell> {
        if self.interval == 0 || self.thumbnail_count == 0 || self.tile_width == 0 || self.tile_height == 0 {
            return None;
        }
        let index = ((pos.max(0.0) * 1000.0 / self.interval as f64) as u64).min(self.thumbnail_count as u64 - 1) as u32;
        let per_sheet = self.tile_width * self.tile_height;
        let cell = index % per_sheet;
        Some(TrickplayCell {
            sheet: index / per_sheet,
            x: (cell % self.tile_width) * self.width,
            y: (cell / self.tile_width) * self.height,
        })
    }

    /// Size in pixels of a full tile sheet.
    pub fn sheet_size(&self) -> (u32, u32) {
        (self.tile_width * self.width, self.tile_height * self.height)
    }
}

/// The resolution to draw from an item's `Trickplay` JSON object (`{sourceId: {width: info}}`): the smallest one at
/// least 240 px wide, else the widest. None when the item has no usable trickplay.
pub fn pick(raw: &serde_json::Value) -> Option<TrickplayInfo> {
    let sources: BTreeMap<String, BTreeMap<String, TrickplayInfo>> = serde_json::from_value(raw.clone()).ok()?;
    let mut all: Vec<TrickplayInfo> = sources.into_values().next()?.into_values().collect();
    all.retain(|i| i.interval > 0 && i.thumbnail_count > 0 && i.tile_width > 0 && i.tile_height > 0);
    all.sort_by_key(|i| i.width);
    all.iter().find(|i| i.width >= 240).or(all.last()).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info() -> TrickplayInfo {
        TrickplayInfo {
            width: 320,
            height: 180,
            tile_width: 10,
            tile_height: 10,
            thumbnail_count: 250,
            interval: 10_000,
        }
    }

    #[test]
    fn first_thumbnail_is_the_origin_of_sheet_zero() {
        assert_eq!(info().locate(0.0), Some(TrickplayCell { sheet: 0, x: 0, y: 0 }));
        assert_eq!(info().locate(9.9), Some(TrickplayCell { sheet: 0, x: 0, y: 0 }));
    }

    #[test]
    fn cells_run_left_to_right_then_down() {
        assert_eq!(info().locate(10.0), Some(TrickplayCell { sheet: 0, x: 320, y: 0 }));
        assert_eq!(info().locate(95.0), Some(TrickplayCell { sheet: 0, x: 9 * 320, y: 0 }));
        assert_eq!(info().locate(100.0), Some(TrickplayCell { sheet: 0, x: 0, y: 180 }));
        assert_eq!(info().locate(375.0), Some(TrickplayCell { sheet: 0, x: 7 * 320, y: 3 * 180 }));
    }

    #[test]
    fn index_100_starts_the_second_sheet() {
        assert_eq!(info().locate(1000.0), Some(TrickplayCell { sheet: 1, x: 0, y: 0 }));
        assert_eq!(info().locate(1230.0), Some(TrickplayCell { sheet: 1, x: 3 * 320, y: 2 * 180 }));
    }

    #[test]
    fn position_is_clamped_to_the_ends() {
        assert_eq!(info().locate(-5.0), info().locate(0.0));
        assert_eq!(info().locate(99_999.0), Some(TrickplayCell { sheet: 2, x: 9 * 320, y: 4 * 180 }));
    }

    #[test]
    fn unusable_info_gives_nothing() {
        assert_eq!(TrickplayInfo { interval: 0, ..info() }.locate(1.0), None);
        assert_eq!(TrickplayInfo { thumbnail_count: 0, ..info() }.locate(1.0), None);
        assert_eq!(TrickplayInfo { tile_width: 0, ..info() }.locate(1.0), None);
    }

    #[test]
    fn pick_prefers_the_smallest_width_of_240_or_more() {
        let raw = serde_json::json!({"src": {
            "160": {"Width":160,"Height":90,"TileWidth":10,"TileHeight":10,"ThumbnailCount":4,"Interval":10000,"Bandwidth":1},
            "320": {"Width":320,"Height":180,"TileWidth":10,"TileHeight":10,"ThumbnailCount":4,"Interval":10000,"Bandwidth":1},
            "640": {"Width":640,"Height":360,"TileWidth":5,"TileHeight":5,"ThumbnailCount":4,"Interval":10000,"Bandwidth":1}
        }});
        assert_eq!(pick(&raw).unwrap().width, 320);
        let small = serde_json::json!({"src": {"160": {"Width":160,"Height":90,"TileWidth":10,"TileHeight":10,"ThumbnailCount":4,"Interval":10000}}});
        assert_eq!(pick(&small).unwrap().width, 160);
        assert!(pick(&serde_json::json!(null)).is_none());
        assert!(pick(&serde_json::json!({})).is_none());
    }
}
