//! Jellyfin DTOs (10.8–10.10). PascalCase on the wire; unknown fields ignored; every field defaulted so a
//! server that leaves one out never breaks decoding (R-API-3).
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
#[serde(rename_all = "PascalCase", default)]
pub struct PublicInfo {
    pub server_name: String,
    pub version: String,
    pub id: String,
}

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(rename_all = "PascalCase", default)]
pub struct PublicUser {
    pub name: String,
    pub id: String,
    pub has_password: bool,
}

/// What a sign-in gives back; stored in settings (R-SET-1).
#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
pub struct Session {
    pub user_id: String,
    pub user_name: String,
    pub token: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DeviceInfo {
    pub name: String,
    pub id: String,
    pub version: String,
}

#[derive(Deserialize, Debug, Clone, PartialEq, Default)]
pub enum ItemKind {
    Movie,
    Series,
    Season,
    Episode,
    CollectionFolder,
    BoxSet,
    Folder,
    #[default]
    #[serde(other)]
    Other,
}

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(rename_all = "PascalCase", default)]
pub struct UserData {
    pub played: bool,
    pub playback_position_ticks: i64,
    pub played_percentage: Option<f64>,
    pub unplayed_item_count: Option<i32>,
    pub is_favorite: bool,
}

#[derive(Deserialize, Debug, Clone, PartialEq, Default)]
pub enum StreamKind {
    Video,
    Audio,
    Subtitle,
    #[default]
    #[serde(other)]
    Other,
}

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(rename_all = "PascalCase", default)]
pub struct MediaStream {
    pub index: i32,
    #[serde(rename = "Type")]
    pub kind: StreamKind,
    pub codec: Option<String>,
    pub language: Option<String>,
    pub title: Option<String>,
    pub display_title: Option<String>,
    pub is_external: bool,
    pub is_default: bool,
    pub is_forced: bool,
    pub delivery_url: Option<String>,
}

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(rename_all = "PascalCase", default)]
pub struct MediaSource {
    pub id: String,
    pub container: Option<String>,
    pub supports_direct_play: bool,
    pub run_time_ticks: Option<i64>,
    pub media_streams: Vec<MediaStream>,
    pub default_audio_stream_index: Option<i32>,
    pub default_subtitle_stream_index: Option<i32>,
}

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(rename_all = "PascalCase", default)]
pub struct Item {
    pub id: String,
    pub name: String,
    #[serde(rename = "Type")]
    pub kind: ItemKind,
    pub collection_type: Option<String>,
    pub overview: Option<String>,
    pub production_year: Option<i32>,
    pub run_time_ticks: Option<i64>,
    pub official_rating: Option<String>,
    pub community_rating: Option<f32>,
    pub genres: Vec<String>,
    /// R-TAG-5: server tags (Rustfin's own start with `rf-`).
    pub tags: Vec<String>,
    pub taglines: Vec<String>,
    pub critic_rating: Option<f32>,
    pub studios: Vec<NameId>,
    pub people: Vec<PersonRef>,
    pub provider_ids: HashMap<String, String>,
    pub chapters: Vec<Chapter>,
    pub is_folder: bool,
    pub premiere_date: Option<String>,
    pub date_created: Option<String>,
    pub local_trailer_count: Option<i32>,
    pub remote_trailers: Vec<MediaUrl>,
    pub special_feature_count: Option<i32>,
    pub part_count: Option<i32>,
    pub series_id: Option<String>,
    pub series_name: Option<String>,
    pub season_id: Option<String>,
    pub index_number: Option<i32>,
    pub parent_index_number: Option<i32>,
    pub child_count: Option<i32>,
    pub primary_image_aspect_ratio: Option<f64>,
    pub image_tags: HashMap<String, String>,
    pub backdrop_image_tags: Vec<String>,
    pub series_primary_image_tag: Option<String>,
    pub parent_backdrop_item_id: Option<String>,
    pub parent_backdrop_image_tags: Vec<String>,
    pub user_data: Option<UserData>,
    pub media_sources: Vec<MediaSource>,
    pub media_streams: Vec<MediaStream>,
}

/// One page of a library (R-API-17).
#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(rename_all = "PascalCase", default)]
pub struct Page {
    pub items: Vec<Item>,
    pub total_record_count: usize,
}

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(rename_all = "PascalCase", default)]
pub struct PlaybackInfo {
    pub media_sources: Vec<MediaSource>,
    pub play_session_id: Option<String>,
}

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(rename_all = "PascalCase", default)]
pub struct QuickConnectState {
    pub secret: String,
    pub code: String,
    pub authenticated: bool,
}

/// Body for /Sessions/Playing, /Progress and /Stopped (R-API-22). Serialised PascalCase; `PlayMethod` is always
/// "DirectPlay" and `CanSeek` true (added by the client).
#[derive(Serialize, Debug, Clone, Default, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct ProgressReport {
    pub item_id: String,
    pub media_source_id: String,
    pub play_session_id: Option<String>,
    pub position_ticks: i64,
    pub is_paused: bool,
    pub audio_stream_index: Option<i32>,
    pub subtitle_stream_index: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageKind {
    Primary,
    Backdrop,
    Thumb,
}

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(rename_all = "PascalCase", default)]
pub struct NameId {
    pub name: String,
    pub id: String,
}

/// A cast or crew entry on an item (`People`).
#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(rename_all = "PascalCase", default)]
pub struct PersonRef {
    pub name: String,
    pub id: String,
    /// Character for actors.
    pub role: Option<String>,
    /// "Actor", "Director", "Writer", "Producer", "GuestStar", …
    #[serde(rename = "Type")]
    pub kind: String,
    pub primary_image_tag: Option<String>,
}

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(rename_all = "PascalCase", default)]
pub struct Chapter {
    pub start_position_ticks: i64,
    pub name: Option<String>,
    pub image_tag: Option<String>,
}

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(rename_all = "PascalCase", default)]
pub struct MediaUrl {
    pub url: String,
    pub name: Option<String>,
}
