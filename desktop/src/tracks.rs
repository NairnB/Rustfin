//! R-PLAY-6: Jellyfin `MediaStream.Index` → mpv track ids. Pure, no mpv.
use jf_core::platform::{PlayRequest, TrackRef};

/// mpv `aid` for a Jellyfin audio index: 1-based position in `req.audio_indexes`, `"no"` for None,
/// `"auto"` when the index is not one of the source's audio streams.
pub fn aid(req: &PlayRequest, track: Option<TrackRef>) -> String {
    match track {
        None => "no".into(),
        Some(t) => {
            req.audio_indexes.iter().position(|&i| i == t).map(|p| (p + 1).to_string()).unwrap_or_else(|| "auto".into())
        }
    }
}

/// mpv `sid` for a Jellyfin subtitle index. Embedded subs take 1-based positions in
/// `req.subtitle_indexes`; external subs follow them, so an external sub at position `p` is
/// `subtitle_indexes.len() + 1 + p`. `"no"` for None, `"auto"` when the index is unknown.
pub fn sid(req: &PlayRequest, track: Option<TrackRef>) -> String {
    match track {
        None => "no".into(),
        Some(t) => {
            if let Some(p) = req.subtitle_indexes.iter().position(|&i| i == t) {
                return (p + 1).to_string();
            }
            if let Some(p) = req.external_subs.iter().position(|s| s.index == t) {
                return (req.subtitle_indexes.len() + 1 + p).to_string();
            }
            "auto".into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jf_core::platform::ExternalSub;

    fn req() -> PlayRequest {
        PlayRequest {
            audio_indexes: vec![2, 3],
            subtitle_indexes: vec![4],
            external_subs: vec![
                ExternalSub { index: 0, url: "u0".into(), title: "t0".into() },
                ExternalSub { index: 5, url: "u5".into(), title: "t5".into() },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn aid_maps_positions() {
        let r = req();
        assert_eq!(aid(&r, Some(2)), "1");
        assert_eq!(aid(&r, Some(3)), "2");
        assert_eq!(aid(&r, None), "no");
        assert_eq!(aid(&r, Some(99)), "auto");
    }

    #[test]
    fn sid_maps_embedded_then_external() {
        let r = req();
        assert_eq!(sid(&r, Some(4)), "1");
        assert_eq!(sid(&r, Some(0)), "2");
        assert_eq!(sid(&r, Some(5)), "3");
        assert_eq!(sid(&r, None), "no");
        assert_eq!(sid(&r, Some(99)), "auto");
    }
}
