//! Builds the player's request from PlaybackInfo + the item page's choices (R-PLAY-1, R-API-30). Pure. Body: packet P1.
use crate::api::{ticks_to_secs, Client, Item, ItemKind, PlaybackInfo, StreamKind};
use crate::platform::{ExternalSub, PlayRequest};
use crate::ui::player::PlayOptions;

/// - url = `client.stream_url(item.id, media_source.id)` of the first media source.
/// - start_secs = 0 when `opts.from_start`, else the item's `UserData.PlaybackPositionTicks` in seconds.
/// - audio_indexes / subtitle_indexes = the **embedded** (not external) audio / subtitle stream indexes, ascending.
/// - external_subs = external subtitle streams with a `client.subtitle_url` (others skipped), ascending by index,
///   title = display_title or language or "Subtitle <index>".
/// - audio = `opts.audio`, else the source's default audio index, else the first audio index.
/// - subtitle = `opts.subtitle` when Some (Some(None) = off), else the source's default subtitle index (None = off).
/// - title = for episodes "Series · S1E2 · Name", else the item name.
///   Returns None when there is no media source.
pub fn build(client: &Client, item: &Item, info: &PlaybackInfo, opts: &PlayOptions) -> Option<PlayRequest> {
    let ms = info.media_sources.first()?;
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
        None => ms.default_subtitle_stream_index,
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
