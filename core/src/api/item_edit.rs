//! A6 (P-ADM-27, P-ITEM-12, P-ITEM-20): admin item controls — edit metadata, refresh, identify, split / merge
//! versions, delete. The edit works on the item's whole JSON so every field it does not show is posted back
//! unchanged (like `update_tags`).
use super::{url_encode, Client, Result, Route, ITEM_FIELDS};
use serde_json::{json, Value};

/// One person row of the editor.
#[derive(Debug, Clone, PartialEq)]
pub struct PersonEdit {
    pub name: String,
    pub kind: String,
    pub role: String,
}

/// The fields the metadata editor shows. `rf_tags` (Rustfin's `rf-` tags) are read-only: they are kept as they are.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ItemEdit {
    pub name: String,
    pub sort_name: String,
    pub year: Option<i32>,
    pub overview: String,
    pub genres: Vec<String>,
    pub tags: Vec<String>,
    pub rf_tags: Vec<String>,
    pub official_rating: String,
    pub community_rating: Option<f32>,
    pub people: Vec<PersonEdit>,
}

fn text(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

fn strings(v: &Value, key: &str) -> Vec<String> {
    match v.get(key) {
        Some(Value::Array(a)) => a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect(),
        _ => Vec::new(),
    }
}

impl ItemEdit {
    pub fn from_json(v: &Value) -> ItemEdit {
        let (rf_tags, tags): (Vec<String>, Vec<String>) =
            strings(v, "Tags").into_iter().partition(|t| t.starts_with("rf-"));
        let people = match v.get("People") {
            Some(Value::Array(a)) => a
                .iter()
                .map(|p| PersonEdit { name: text(p, "Name"), kind: text(p, "Type"), role: text(p, "Role") })
                .collect(),
            _ => Vec::new(),
        };
        ItemEdit {
            name: text(v, "Name"),
            sort_name: text(v, "ForcedSortName"),
            year: v.get("ProductionYear").and_then(Value::as_i64).map(|y| y as i32),
            overview: text(v, "Overview"),
            genres: strings(v, "Genres"),
            tags,
            rf_tags,
            official_rating: text(v, "OfficialRating"),
            community_rating: v.get("CommunityRating").and_then(Value::as_f64).map(|r| r as f32),
            people,
        }
    }
}

fn set(map: &mut serde_json::Map<String, Value>, key: &str, v: Value) {
    map.insert(key.into(), v);
}

/// Write the edit into the item JSON; every key the edit does not own stays as it was. People that did not
/// change keep their server fields (ids, image tags).
pub fn apply_edit(value: &mut Value, e: &ItemEdit) {
    let old_people = value.get("People").and_then(Value::as_array).cloned().unwrap_or_default();
    let Value::Object(map) = value else { return };
    set(map, "Name", json!(e.name));
    set(map, "ForcedSortName", json!(e.sort_name));
    set(map, "ProductionYear", e.year.map_or(Value::Null, |y| json!(y)));
    set(map, "Overview", json!(e.overview));
    set(map, "Genres", json!(e.genres));
    let mut tags = e.tags.clone();
    tags.extend(e.rf_tags.iter().cloned());
    set(map, "Tags", json!(tags));
    set(map, "OfficialRating", if e.official_rating.is_empty() { Value::Null } else { json!(e.official_rating) });
    set(map, "CommunityRating", e.community_rating.map_or(Value::Null, |r| json!(r)));
    let people: Vec<Value> = e
        .people
        .iter()
        .map(|p| {
            let same = old_people
                .iter()
                .find(|o| text(o, "Name") == p.name && text(o, "Type") == p.kind && text(o, "Role") == p.role);
            match same {
                Some(o) => o.clone(),
                None => {
                    let mut o = json!({ "Name": p.name, "Type": p.kind });
                    if !p.role.is_empty() {
                        o["Role"] = json!(p.role);
                    }
                    o
                }
            }
        })
        .collect();
    set(map, "People", Value::Array(people));
}

/// "Drama, Sci-Fi" → ["Drama", "Sci-Fi"] (blank entries dropped).
pub fn split_list(s: &str) -> Vec<String> {
    s.split(',').map(str::trim).filter(|x| !x.is_empty()).map(str::to_string).collect()
}

pub fn join_list(v: &[String]) -> String {
    v.join(", ")
}

/// "Name | Type | Role" → a person; the type defaults to Actor, a blank line is no person.
pub fn parse_person(line: &str) -> Option<PersonEdit> {
    let mut parts = line.split('|').map(str::trim);
    let name = parts.next().filter(|n| !n.is_empty())?.to_string();
    let kind = parts.next().filter(|k| !k.is_empty()).unwrap_or("Actor").to_string();
    let role = parts.next().unwrap_or_default().to_string();
    Some(PersonEdit { name, kind, role })
}

pub fn person_line(p: &PersonEdit) -> String {
    if p.role.is_empty() {
        format!("{} | {}", p.name, p.kind)
    } else {
        format!("{} | {} | {}", p.name, p.kind, p.role)
    }
}

/// Community rating text: blank = none, otherwise 0–10.
pub fn parse_rating(s: &str) -> std::result::Result<Option<f32>, String> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(None);
    }
    match s.parse::<f32>() {
        Ok(r) if (0.0..=10.0).contains(&r) => Ok(Some(r)),
        _ => Err("Community rating must be a number from 0 to 10.".into()),
    }
}

/// Year text: blank = none, otherwise 1–9999.
pub fn parse_year(s: &str) -> std::result::Result<Option<i32>, String> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(None);
    }
    match s.parse::<i32>() {
        Ok(y) if (1..=9999).contains(&y) => Ok(Some(y)),
        _ => Err("Year must be a number such as 1999.".into()),
    }
}

/// How hard "Refresh metadata" looks (the jellyfin-web choices).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RefreshMode {
    /// Scan for new and updated files.
    Scan,
    /// Search for missing metadata.
    Missing,
    /// Replace all metadata.
    Replace,
}

pub fn refresh_path(id: &str, mode: RefreshMode, replace_images: bool) -> String {
    let (how, all) = match mode {
        RefreshMode::Scan => ("Default", false),
        RefreshMode::Missing => ("FullRefresh", false),
        RefreshMode::Replace => ("FullRefresh", true),
    };
    format!(
        "/Items/{id}/Refresh?Recursive=true&ImageRefreshMode={how}&MetadataRefreshMode={how}&ReplaceAllImages={replace_images}&ReplaceAllMetadata={all}"
    )
}

pub fn merge_path(ids: &[String]) -> String {
    format!("/Videos/MergeVersions?Ids={}", ids.join(","))
}

/// The server's web page for an item (what "Copy link" copies).
pub fn web_link(base: &str, id: &str, server_id: Option<&str>) -> String {
    match server_id {
        Some(s) => format!("{base}/web/index.html#!/details?id={id}&serverId={s}"),
        None => format!("{base}/web/index.html#!/details?id={id}"),
    }
}

/// The remote-search kind for an item kind name (`None` = Identify does not apply).
pub fn identify_kind(kind: &str) -> Option<&'static str> {
    match kind {
        "Movie" => Some("Movie"),
        "Series" => Some("Series"),
        _ => None,
    }
}

/// One result of an Identify search. `raw` is posted back unchanged to apply it.
#[derive(Debug, Clone, PartialEq)]
pub struct RemoteMatch {
    pub raw: Value,
}

impl RemoteMatch {
    pub fn from_json(raw: Value) -> RemoteMatch {
        RemoteMatch { raw }
    }

    pub fn label(&self) -> String {
        let mut s = text(&self.raw, "Name");
        if let Some(y) = self.raw.get("ProductionYear").and_then(Value::as_i64) {
            s.push_str(&format!(" ({y})"));
        }
        let p = text(&self.raw, "SearchProviderName");
        if !p.is_empty() {
            s.push_str(&format!(" · {p}"));
        }
        s
    }

    pub fn overview(&self) -> String {
        text(&self.raw, "Overview")
    }
}

impl Client {
    /// P-ADM-27: the whole item JSON (editor fields included) for `edit_item`.
    pub fn item_json(&self, item_id: &str) -> Result<Value> {
        let uid = self.user_id()?;
        let new = format!("/Items/{item_id}?userId={uid}&Fields={ITEM_FIELDS}");
        let legacy = format!("/Users/{uid}/Items/{item_id}?Fields={ITEM_FIELDS}");
        self.get_with_fallback(Route::Item, &new, &legacy)
    }

    /// P-ADM-27: save the editor's fields; re-reads the item first so nothing else changes. Non-admin → `Forbidden`.
    pub fn save_item_edit(&self, item_id: &str, edit: &ItemEdit) -> Result<()> {
        let mut value = self.item_json(item_id)?;
        apply_edit(&mut value, edit);
        self.post_ok(&format!("/Items/{item_id}"), &value)
    }

    /// P-ADM-27: refresh an item's metadata.
    pub fn refresh_item(&self, item_id: &str, mode: RefreshMode, replace_images: bool) -> Result<()> {
        self.post_empty_ok(&refresh_path(item_id, mode, replace_images))
    }

    /// P-ADM-27: Identify — search the metadata providers (`kind` from `identify_kind`).
    pub fn remote_search(&self, kind: &str, item_id: &str, name: &str, year: Option<i32>) -> Result<Vec<RemoteMatch>> {
        let body = json!({ "SearchInfo": { "Name": name, "Year": year, "ProviderIds": {} }, "ItemId": item_id });
        let raw: Value = self.post_json(&format!("/Items/RemoteSearch/{kind}"), &body)?;
        Ok(raw.as_array().cloned().unwrap_or_default().into_iter().map(RemoteMatch::from_json).collect())
    }

    /// P-ADM-27: Identify — apply the chosen match to the item.
    pub fn apply_remote_match(&self, item_id: &str, m: &RemoteMatch, replace_images: bool) -> Result<()> {
        self.post_ok(&format!("/Items/RemoteSearch/Apply/{item_id}?ReplaceAllImages={replace_images}"), &m.raw)
    }

    /// P-ITEM-12: other films with this title (candidates for "Merge versions").
    pub fn merge_candidates(&self, item_id: &str, name: &str) -> Result<Vec<super::Item>> {
        let uid = self.user_id()?;
        let path = format!(
            "/Items?userId={uid}&Recursive=true&IncludeItemTypes=Movie&SearchTerm={}&Fields=MediaSources,ProductionYear&Limit=50",
            url_encode(name)
        );
        let page: super::Page = self.get_json(&path)?;
        Ok(page.items.into_iter().filter(|i| i.id != item_id).collect())
    }

    /// P-ITEM-12: merge films into one item with several versions (`item_ids` includes the one to keep first).
    pub fn merge_versions(&self, item_ids: &[String]) -> Result<()> {
        self.post_empty_ok(&merge_path(item_ids))
    }

    /// P-ITEM-12: split a multi-version item back into separate items.
    pub fn split_versions(&self, item_id: &str) -> Result<()> {
        self.delete_ok(&format!("/Videos/{item_id}/AlternateSources"))
    }
}
