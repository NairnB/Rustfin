//! Builds the player's request from PlaybackInfo + the item page's choices (R-PLAY-1, R-API-30). Pure. Body: packet P1.
use crate::api::{
    ticks_to_secs, with_stream_indexes, Client, Item, ItemKind, MediaSource, PlaybackInfo, PlaybackPlan, StreamKind,
};
use crate::platform::{ExternalSub, PlayRequest};
use crate::settings::{SubMode, SubPick, SubPrefs};
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
        None => default_subtitle(prefs, ms, audio, opts.remembered_subtitle.as_ref()),
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
    build_plan_burnt(client, item, plan, opts, prefs).map(|(req, _)| req)
}

/// C3-SUBS: `build_plan`, also giving the embedded subtitle the server burns into the transcode (R-SUBS-2 / 5 / 6) —
/// the request's own `subtitle` is then None (mpv has nothing to load), so the player keeps the burnt index itself.
pub fn build_plan_burnt(
    client: &Client,
    item: &Item,
    plan: &PlaybackPlan,
    opts: &PlayOptions,
    prefs: &SubPrefs,
) -> Option<(PlayRequest, Option<i32>)> {
    build_plan_burnt_on(true, client, item, plan, opts, prefs)
}

/// FIX-3 T1: `build_plan_burnt` for a backend that does (`renders_subtitles`) or does not draw subtitles itself.
/// One that does not has every chosen subtitle, external ones too, burnt in by the server.
pub fn build_plan_burnt_on(
    renders_subtitles: bool,
    client: &Client,
    item: &Item,
    plan: &PlaybackPlan,
    opts: &PlayOptions,
    prefs: &SubPrefs,
) -> Option<(PlayRequest, Option<i32>)> {
    let mut req = build_with_prefs(client, item, &plan.info, opts, prefs)?;
    let mut burnt = None;
    if let Some(url) = &plan.transcoding_url {
        // R-SUBS-2: the server burns in the embedded subtitle the url names, -1 = none (an external one loads here).
        // R-SUBS-6: only a subtitle stream the chosen source really has is burnt; any other choice is sent as none.
        let source = pick_source(&plan.info, opts.media_source_id.as_deref());
        burnt = req.subtitle.filter(|s| {
            (!renders_subtitles || !req.external_subs.iter().any(|e| e.index == *s))
                && source.is_some_and(|ms| {
                    ms.media_streams.iter().any(|m| {
                        m.index == *s && m.kind == StreamKind::Subtitle && (!m.is_external || !renders_subtitles)
                    })
                })
        });
        req.url = with_stream_indexes(url, req.audio, Some(burnt.unwrap_or(-1)), burnt.is_some());
        req.audio_indexes = req.audio.into_iter().collect();
        req.subtitle = req.subtitle.filter(|s| renders_subtitles && req.external_subs.iter().any(|e| e.index == *s));
        req.subtitle_indexes.clear();
    }
    Some((req, burnt))
}

/// G8 / P-ITEM-11: the media source with id `want`, else the first one.
pub fn pick_source<'a>(info: &'a PlaybackInfo, want: Option<&str>) -> Option<&'a MediaSource> {
    want.and_then(|id| info.media_sources.iter().find(|m| m.id == id)).or_else(|| info.media_sources.first())
}

/// FIX-4 (24 Oct): the subtitle a viewer gets when they have not picked one — the film page, the player, Home
/// Resume, Poster Flow and next/prev all ask this, with the chosen audio track (None = the source's default audio) and
/// the viewer's own earlier pick (`remembered`, kept by the app while their RememberSubtitleSelections is on).
/// Mode `Default` ignores the file's and the server's default / forced flags and `DefaultSubtitleStreamIndex`: a
/// remembered Off, or a remembered pick that still finds its subtitle stream (same index and language, else — another
/// episode — the same language and forced flag), wins; else English (or unknown) audio → Off; any other audio →
/// the English forced subtitle if the film has one, else the first full English one, else the viewer's preferred
/// language, else Off. The other modes follow `apply_sub_prefs`.
pub fn default_subtitle(
    prefs: &SubPrefs,
    ms: &MediaSource,
    audio: Option<i32>,
    remembered: Option<&SubPick>,
) -> Option<i32> {
    if prefs.mode != SubMode::Default {
        return apply_sub_prefs(prefs, ms);
    }
    let subs = || ms.media_streams.iter().filter(|s| s.kind == StreamKind::Subtitle);
    if let Some(p) = remembered {
        if p.index < 0 {
            return None;
        }
        let lang = |s: &crate::api::MediaStream| {
            s.language.as_deref().map(str::to_ascii_lowercase) == p.language.as_deref().map(str::to_ascii_lowercase)
        };
        let found = subs()
            .find(|s| s.index == p.index && lang(s))
            .or_else(|| subs().find(|s| p.language.is_some() && lang(s) && s.is_forced == p.forced));
        if let Some(s) = found {
            return Some(s.index);
        }
    }
    let audio_streams = || ms.media_streams.iter().filter(|s| s.kind == StreamKind::Audio);
    let chosen = audio
        .or(ms.default_audio_stream_index)
        .and_then(|i| audio_streams().find(|s| s.index == i))
        .or_else(|| audio_streams().next());
    if chosen.is_none_or(|a| is_english(a.language.as_deref())) {
        return None;
    }
    let english = || subs().filter(|s| is_english(s.language.as_deref()) && s.language.is_some());
    english()
        .find(|s| s.is_forced)
        .or_else(|| english().next())
        .or_else(|| {
            let want = prefs.language.trim();
            subs().find(|s| !want.is_empty() && s.language.as_deref().is_some_and(|l| l.eq_ignore_ascii_case(want)))
        })
        .map(|s| s.index)
}

/// FIX-4 R5: the subtitle choice (`None` = not chosen yet, `Some(None)` = Off) after the audio track becomes `audio`:
/// a subtitle the viewer picked stays, otherwise the default for that audio is re-applied. Page and player share it.
pub fn subtitle_after_audio_pick(
    prefs: &SubPrefs,
    ms: &MediaSource,
    audio: i32,
    remembered: Option<&SubPick>,
    viewer_picked: bool,
    current: Option<Option<i32>>,
) -> Option<Option<i32>> {
    if viewer_picked {
        return current;
    }
    Some(default_subtitle(prefs, ms, Some(audio), remembered))
}

/// English, or no language to go on (unset, "und", "unk") — both count as English for the subtitle default.
fn is_english(lang: Option<&str>) -> bool {
    let l = lang.unwrap_or("").trim().to_ascii_lowercase();
    matches!(l.as_str(), "" | "eng" | "en" | "english" | "und" | "unk" | "undetermined")
        || l.starts_with("en-")
        || l.starts_with("en_")
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

/// C3-SUBS / R-SUBS-1: the `(AudioStreamIndex, SubtitleStreamIndex)` a PlaybackInfo request names. Audio as chosen;
/// subtitle: an embedded stream → its index, off or an external one (the client loads it) → -1, no choice → none.
/// With no `info` yet (the first request) a chosen subtitle index is sent as given.
pub fn request_indexes(
    info: Option<&PlaybackInfo>,
    source_id: Option<&str>,
    audio: Option<i32>,
    subtitle: Option<Option<i32>>,
) -> (Option<i32>, Option<i32>) {
    request_indexes_on(true, info, source_id, audio, subtitle)
}

/// FIX-3 T1: `request_indexes` for a backend that cannot draw subtitles: an external subtitle is named too, as the
/// server has to burn it in.
pub fn request_indexes_on(
    renders_subtitles: bool,
    info: Option<&PlaybackInfo>,
    source_id: Option<&str>,
    audio: Option<i32>,
    subtitle: Option<Option<i32>>,
) -> (Option<i32>, Option<i32>) {
    let sub = subtitle.map(|s| match s {
        None => -1,
        Some(i) => {
            let external = renders_subtitles
                && info
                    .and_then(|inf| pick_source(inf, source_id))
                    .is_some_and(|ms| ms.media_streams.iter().any(|m| m.index == i && m.is_external));
            if external {
                -1
            } else {
                i
            }
        }
    });
    (audio, sub)
}

/// FIX-3 T1 / T2: a backend that cannot draw subtitles must transcode whenever one is chosen; with none (Off) it
/// plays direct.
pub fn must_transcode(renders_subtitles: bool, subtitle: Option<i32>) -> bool {
    !renders_subtitles && subtitle.is_some_and(|s| s >= 0)
}
