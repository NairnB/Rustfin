//! Builds the player's request from PlaybackInfo + the item page's choices (R-PLAY-1, R-API-30). Pure. Body: packet P1.
use crate::api::{ticks_to_secs, Client, Item, ItemKind, MediaSource, PlaybackInfo, PlaybackPlan, StreamKind};
use crate::platform::{ExternalSub, PlayRequest};
use crate::settings::{SubMode, SubPrefs};
use crate::ui::player::PlayOptions;

/// - url = `client.stream_url(item.id, media_source.id)` of the source named by `opts.media_source_id` (P-ITEM-11),
///   else the first media source.
/// - start_secs = 0 when `opts.from_start`, else the item's `UserData.PlaybackPositionTicks` in seconds.
/// - audio_indexes / subtitle_indexes = the **embedded** (not external) audio / subtitle stream indexes, ascending.
/// - external_subs = external subtitle streams with a `client.subtitle_url` (others skipped), ascending by index,
///   title = display_title or language or "Subtitle <index>".
/// - audio = `opts.audio`, else the source's default audio index, else the first audio index.
/// - subtitle = `opts.subtitle` when Some (Some(None) = off), else the user's subtitle preferences (P-USER-5).
/// - title = for episodes "Series · S1E2 · Name", else the item name.
///   Returns None when there is no media source.
pub fn build(client: &Client, item: &Item, info: &PlaybackInfo, opts: &PlayOptions) -> Option<PlayRequest> {
    build_with_prefs(client, item, info, opts, &SubPrefs::default())
}

/// As `build`, applying `prefs` (the user's subtitle mode + language) when the item page did not choose a
/// subtitle stream (`opts.subtitle == None`). P-USER-5.
pub fn build_with_prefs(
    client: &Client,
    item: &Item,
    info: &PlaybackInfo,
    opts: &PlayOptions,
    prefs: &SubPrefs,
) -> Option<PlayRequest> {
    let ms = pick_source(info, opts.media_source_id.as_deref())?;
    let url = client.stream_url(&item.id, &ms.id);

    let mut audio_indexes = vec![];
    let mut subtitle_indexes = vec![];
    let mut external_subs = vec![];
    for s in &ms.media_streams {
        match s.kind {
            StreamKind::Audio if !s.is_external => audio_indexes.push(s.index),
            StreamKind::Subtitle if s.is_external => {
                if let Some(u) = client.subtitle_url(&item.id, &ms.id, s) {
                    let title = s
                        .display_title
                        .clone()
                        .filter(|t| !t.is_empty())
                        .or_else(|| s.language.clone())
                        .unwrap_or_else(|| format!("Subtitle {}", s.index));
                    external_subs.push(ExternalSub { index: s.index, url: u, title });
                }
            }
            StreamKind::Subtitle => subtitle_indexes.push(s.index),
            _ => {}
        }
    }
    audio_indexes.sort_unstable();
    subtitle_indexes.sort_unstable();
    external_subs.sort_by_key(|s| s.index);

    let start_secs = if opts.from_start {
        0.0
    } else {
        ticks_to_secs(item.user_data.as_ref().map(|u| u.playback_position_ticks).unwrap_or(0))
    };

    let audio = opts.audio.or(ms.default_audio_stream_index).or_else(|| audio_indexes.first().copied());
    let subtitle = match opts.subtitle {
        Some(s) => s,
        None => apply_sub_prefs(prefs, ms),
    };

    let title = if item.kind == ItemKind::Episode {
        let series = item.series_name.clone().unwrap_or_default();
        let season = item.parent_index_number.unwrap_or(0);
        let number = item.index_number.unwrap_or(0);
        format!("{series} · S{season}E{number} · {}", item.name)
    } else {
        item.name.clone()
    };

    Some(PlayRequest { url, title, start_secs, audio_indexes, subtitle_indexes, external_subs, audio, subtitle })
}

/// G6 / P-PLAY-13: as `build_with_prefs` for a `PlaybackPlan`. With a `transcoding_url` the stream is that HLS url
/// and carries one audio track (the chosen one) and no embedded subtitles; external subtitles still load. With
/// none it is exactly `build_with_prefs` on the plan's info.
pub fn build_plan(
    client: &Client,
    item: &Item,
    plan: &PlaybackPlan,
    opts: &PlayOptions,
    prefs: &SubPrefs,
) -> Option<PlayRequest> {
    let mut req = build_with_prefs(client, item, &plan.info, opts, prefs)?;
    if let Some(url) = &plan.transcoding_url {
        req.url = url.clone();
        req.audio_indexes = req.audio.into_iter().collect();
        if req.subtitle.is_some_and(|s| !req.external_subs.iter().any(|e| e.index == s)) {
            req.subtitle = None;
        }
        req.subtitle_indexes.clear();
    }
    Some(req)
}

/// G8 / P-ITEM-11: the media source with id `want`, else the first one.
pub fn pick_source<'a>(info: &'a PlaybackInfo, want: Option<&str>) -> Option<&'a MediaSource> {
    want.and_then(|id| info.media_sources.iter().find(|m| m.id == id)).or_else(|| info.media_sources.first())
}

/// P-USER-5: which subtitle stream to use when the item page did not choose one. `Always` → the first sub in
/// the preferred language, else the first; `OnlyForced` → the first forced sub (off when there is none);
/// `None` → off; `Default` → the source's default subtitle index.
pub fn apply_sub_prefs(prefs: &SubPrefs, ms: &MediaSource) -> Option<i32> {
    let subs: Vec<_> = ms.media_streams.iter().filter(|s| s.kind == StreamKind::Subtitle).collect();
    match prefs.mode {
        SubMode::None => None,
        SubMode::OnlyForced => subs.iter().find(|s| s.is_forced).map(|s| s.index),
        SubMode::Always => {
            let lang = prefs.language.trim();
            if !lang.is_empty() {
                subs.iter()
                    .find(|s| s.language.as_deref().map(|l| l.eq_ignore_ascii_case(lang)).unwrap_or(false))
                    .map(|s| s.index)
                    .or_else(|| subs.first().map(|s| s.index))
            } else {
                subs.first().map(|s| s.index)
            }
        }
        SubMode::Default => ms.default_subtitle_stream_index,
    }
}
