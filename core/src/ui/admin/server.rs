//! Admin server settings (P-ADM-3, 4, 16..20): General, Branding, Transcoding, Resume, Streaming, Trickplay and
//! Networking. Every page is one list of fields over a config JSON. Save reads the config again, changes only the
//! fields that were edited and posts the whole thing back, so everything the page doesn't show (and anything edited
//! elsewhere meanwhile) stays as the server has it.
use super::libraries::FormState;
use super::users::{fail, skeleton_rows};
use crate::api::gen_admin::SystemInfo;
use crate::api::{ApiError, Result};
use crate::app::{Cx, Screen};
use crate::platform::Key;
use crate::ui::parts;
use serde_json::Value;

// ---------- pure helpers (unit-tested below) ----------

#[derive(Clone, Copy)]
enum Field {
    /// (path, label)
    Toggle(&'static str, &'static str),
    /// (path, label, hint)
    Text(&'static str, &'static str, &'static str),
    /// A whole number: (path, label, hint, unit divisor — shown value = stored / divisor, min, max shown).
    Number(&'static str, &'static str, &'static str, i64, i64, i64),
    /// (path, label, [(stored value, label)]). A stored value not in the list is offered as an extra choice.
    Choice(&'static str, &'static str, &'static [(&'static str, &'static str)]),
    /// A list typed as comma-separated values: (path, label, hint, numbers?).
    List(&'static str, &'static str, &'static str, bool),
    /// One switch that adds / removes an entry of a list: (path, label, entry).
    InList(&'static str, &'static str, &'static str),
    /// The server's cache folder (read-only, from `/System/Info`).
    CachePath(&'static str),
}

impl Field {
    fn path(&self) -> Option<&'static str> {
        match self {
            Field::Toggle(p, ..)
            | Field::Text(p, ..)
            | Field::Number(p, ..)
            | Field::Choice(p, ..)
            | Field::List(p, ..)
            | Field::InList(p, ..) => Some(p),
            Field::CachePath(_) => None,
        }
    }
}

fn get_path<'a>(v: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').try_fold(v, |v, k| v.get(k))
}

fn set_path(v: &mut Value, path: &str, new: Value) {
    let mut cur = v;
    let mut parts = path.split('.').peekable();
    while let Some(k) = parts.next() {
        if !cur.is_object() {
            *cur = Value::Object(Default::default());
        }
        let Some(o) = cur.as_object_mut() else { return };
        if parts.peek().is_none() {
            o.insert(k.to_string(), new);
            return;
        }
        cur = o.entry(k.to_string()).or_insert_with(|| Value::Object(Default::default()));
    }
}

fn cfg_bool(v: &Value, path: &str) -> bool {
    get_path(v, path).and_then(Value::as_bool).unwrap_or(false)
}

fn cfg_str(v: &Value, path: &str) -> String {
    get_path(v, path).and_then(Value::as_str).unwrap_or("").to_string()
}

fn in_list(v: &Value, path: &str, entry: &str) -> bool {
    get_path(v, path).and_then(Value::as_array).is_some_and(|a| a.iter().any(|e| e.as_str() == Some(entry)))
}

fn toggle_in_list(v: &mut Value, path: &str, entry: &str) {
    let mut list = get_path(v, path).and_then(Value::as_array).cloned().unwrap_or_default();
    match list.iter().position(|e| e.as_str() == Some(entry)) {
        Some(i) => {
            list.remove(i);
        }
        None => list.push(Value::String(entry.to_string())),
    }
    set_path(v, path, Value::Array(list));
}

/// A stored whole number as shown: divided by the unit (a bitrate in bits shows as Mbps), no trailing zeros.
fn number_text(v: &Value, path: &str, unit: i64) -> String {
    let Some(n) = get_path(v, path).and_then(Value::as_i64) else { return String::new() };
    if unit <= 1 || n % unit == 0 {
        return (n / unit.max(1)).to_string();
    }
    let s = format!("{:.3}", n as f64 / unit as f64);
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn parse_number(text: &str, unit: i64, min: i64, max: i64) -> Option<i64> {
    let x: f64 = text.trim().parse().ok()?;
    if !x.is_finite() {
        return None;
    }
    let n = (x * unit.max(1) as f64).round() as i64;
    (n >= min * unit.max(1) && n <= max * unit.max(1)).then_some(n)
}

fn list_text(v: &Value, path: &str) -> String {
    let Some(a) = get_path(v, path).and_then(Value::as_array) else { return String::new() };
    a.iter().map(|e| e.as_str().map(str::to_string).unwrap_or_else(|| e.to_string())).collect::<Vec<_>>().join(", ")
}

fn parse_list(text: &str, numbers: bool) -> Option<Value> {
    let items = text.split(',').map(str::trim).filter(|s| !s.is_empty());
    let out: Option<Vec<Value>> =
        items.map(|s| if numbers { s.parse::<i64>().ok().map(Value::from) } else { Some(Value::from(s)) }).collect();
    out.map(Value::Array)
}

/// The options of a choice: the page's list, plus the stored value when it isn't one of them.
fn choice_options(v: &Value, path: &str, opts: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = opts.iter().map(|(v, l)| (v.to_string(), l.to_string())).collect();
    let cur = cfg_str(v, path);
    if !cur.is_empty() && !out.iter().any(|(v, _)| *v == cur) {
        out.push((cur.clone(), cur));
    }
    out
}

/// Every field that differs between what was loaded and what the form now holds, as (path, new value).
/// An empty text that was not set on the server stays unset.
fn changes(loaded: &Value, edited: &Value, fields: &[Field]) -> Vec<(&'static str, Value)> {
    let mut out = vec![];
    for f in fields {
        let Some(p) = f.path() else { continue };
        let (old, new) = (get_path(loaded, p), get_path(edited, p));
        let unset_empty = old.is_none() && new.and_then(Value::as_str) == Some("");
        if old != new && !unset_empty {
            if let Some(n) = new {
                out.push((p, n.clone()));
            }
        }
    }
    out
}

fn apply(fresh: &mut Value, changes: &[(&'static str, Value)]) {
    for (p, v) in changes {
        set_path(fresh, p, v.clone());
    }
}

// ---------- the pages ----------

#[derive(Clone, Copy)]
struct Kind {
    screen: &'static str,
    title: &'static str,
    /// `None` is `/System/Configuration`; otherwise `/System/Configuration/<section>`.
    section: Option<&'static str>,
    note: Option<&'static str>,
    fields: &'static [Field],
}

const HW_ACCEL: &[(&str, &str)] = &[
    ("none", "None"),
    ("amf", "AMD AMF"),
    ("nvenc", "Nvidia NVENC"),
    ("qsv", "Intel Quick Sync (QSV)"),
    ("vaapi", "Video Acceleration API (VAAPI)"),
    ("rkmpp", "Rockchip MPP"),
    ("videotoolbox", "Apple VideoToolbox"),
    ("v4l2m2m", "Video4Linux2"),
];
const TONE_ALGO: &[(&str, &str)] = &[
    ("none", "None"),
    ("clip", "Clip"),
    ("linear", "Linear"),
    ("gamma", "Gamma"),
    ("reinhard", "Reinhard"),
    ("hable", "Hable"),
    ("mobius", "Mobius"),
    ("bt2390", "BT.2390"),
];
const TONE_MODE: &[(&str, &str)] =
    &[("auto", "Auto"), ("max", "Max"), ("rgb", "RGB"), ("lum", "Luminance"), ("itp", "ITP")];
const TONE_RANGE: &[(&str, &str)] = &[("auto", "Auto"), ("tv", "TV"), ("pc", "PC")];
const SCAN: &[(&str, &str)] = &[("NonBlocking", "Non-blocking"), ("Blocking", "Blocking")];
const PRIORITY: &[(&str, &str)] = &[
    ("High", "High"),
    ("AboveNormal", "Above normal"),
    ("Normal", "Normal"),
    ("BelowNormal", "Below normal"),
    ("Idle", "Idle"),
];

const GENERAL: Kind = Kind {
    screen: "servergeneral",
    title: "General",
    section: None,
    note: None,
    fields: &[
        Field::Text("ServerName", "Server name", "Name shown to clients"),
        Field::Text("UICulture", "Display language", "Language code, e.g. en-GB"),
        Field::Text("MetadataPath", "Metadata folder", "Server folder (empty = default)"),
        Field::CachePath("Cache folder"),
        Field::Toggle("QuickConnectAvailable", "Allow Quick Connect"),
        Field::Number("LibraryMonitorDelay", "Library monitor delay (seconds)", "e.g. 60", 1, 0, 3600),
    ],
};

const BRANDING: Kind = Kind {
    screen: "serverbranding",
    title: "Branding",
    section: Some("branding"),
    note: None,
    fields: &[
        Field::Text("LoginDisclaimer", "Login disclaimer", "Shown on the sign-in page"),
        Field::Text("CustomCss", "Custom CSS", "One line of CSS for the web client"),
        Field::Toggle("SplashscreenEnabled", "Show the splash screen"),
    ],
};

const TRANSCODING: Kind = Kind {
    screen: "playbacktranscoding",
    title: "Transcoding",
    section: Some("encoding"),
    note: Some("Applies to new streams. The server needs the hardware you pick."),
    fields: &[
        Field::Choice("HardwareAccelerationType", "Hardware acceleration", HW_ACCEL),
        Field::Text("VaapiDevice", "VAAPI device path", "e.g. /dev/dri/renderD128"),
        Field::Text("QsvDevice", "QSV device path", "Empty = default"),
        Field::Toggle("EnableHardwareEncoding", "Use hardware encoding"),
        Field::Toggle("AllowHevcEncoding", "Allow encoding in HEVC"),
        Field::Toggle("AllowAv1Encoding", "Allow encoding in AV1"),
        Field::InList("HardwareDecodingCodecs", "Hardware decode H.264", "h264"),
        Field::InList("HardwareDecodingCodecs", "Hardware decode HEVC", "hevc"),
        Field::InList("HardwareDecodingCodecs", "Hardware decode MPEG-2", "mpeg2video"),
        Field::InList("HardwareDecodingCodecs", "Hardware decode VC-1", "vc1"),
        Field::InList("HardwareDecodingCodecs", "Hardware decode VP8", "vp8"),
        Field::InList("HardwareDecodingCodecs", "Hardware decode VP9", "vp9"),
        Field::InList("HardwareDecodingCodecs", "Hardware decode AV1", "av1"),
        Field::Number("EncodingThreadCount", "Encoding threads (-1 = automatic)", "-1 or a count", 1, -1, 256),
        Field::Toggle("EnableThrottling", "Throttle transcodes"),
        Field::Number("ThrottleDelaySeconds", "Throttle delay (seconds)", "e.g. 180", 1, 10, 3600),
        Field::Number("H264Crf", "H.264 quality (CRF)", "0–51, lower is better", 1, 0, 51),
        Field::Number("H265Crf", "H.265 quality (CRF)", "0–51, lower is better", 1, 0, 51),
        Field::Toggle("EnableTonemapping", "Tone mapping"),
        Field::Choice("TonemappingAlgorithm", "Tone mapping algorithm", TONE_ALGO),
        Field::Choice("TonemappingMode", "Tone mapping mode", TONE_MODE),
        Field::Choice("TonemappingRange", "Tone mapping range", TONE_RANGE),
    ],
};

const RESUME: Kind = Kind {
    screen: "playbackresume",
    title: "Resume",
    section: None,
    note: None,
    fields: &[
        Field::Number("MinResumePct", "Resume from (percent watched)", "0–100", 1, 0, 100),
        Field::Number("MaxResumePct", "Counts as watched after (percent)", "0–100", 1, 0, 100),
        Field::Number("MinResumeDurationSeconds", "Shortest video that can resume (seconds)", "e.g. 300", 1, 0, 86_400),
    ],
};

const STREAMING: Kind = Kind {
    screen: "playbackstreaming",
    title: "Streaming",
    section: None,
    note: Some("Applies to clients outside your network. 0 = no limit."),
    fields: &[Field::Number(
        "RemoteClientBitrateLimit",
        "Internet streaming limit (Mbps)",
        "0 = no limit",
        1_000_000,
        0,
        1000,
    )],
};

const TRICKPLAY: Kind = Kind {
    screen: "playbacktrickplay",
    title: "Trickplay",
    section: None,
    note: Some("Preview pictures for the seek bar. Changes apply to the next generation."),
    fields: &[
        Field::Toggle("TrickplayOptions.EnableHwAcceleration", "Hardware decoding"),
        Field::Toggle("TrickplayOptions.EnableHwEncoding", "Hardware encoding"),
        Field::Toggle("TrickplayOptions.EnableKeyFrameOnlyExtraction", "Key frames only"),
        Field::Choice("TrickplayOptions.ScanBehavior", "Image generation", SCAN),
        Field::Choice("TrickplayOptions.ProcessPriority", "Process priority", PRIORITY),
        Field::Number("TrickplayOptions.Interval", "Image interval (milliseconds)", "e.g. 10000", 1, 1, 3_600_000),
        Field::List("TrickplayOptions.WidthResolutions", "Image widths (pixels)", "e.g. 320, 640", true),
        Field::Number("TrickplayOptions.TileWidth", "Tile columns", "e.g. 10", 1, 1, 100),
        Field::Number("TrickplayOptions.TileHeight", "Tile rows", "e.g. 10", 1, 1, 100),
        Field::Number("TrickplayOptions.JpegQuality", "JPEG quality", "1–100", 1, 1, 100),
        Field::Number("TrickplayOptions.Qscale", "Quality scale (ffmpeg -q:v)", "2–31", 1, 2, 31),
        Field::Number("TrickplayOptions.ProcessThreads", "Threads (0 = automatic)", "0 or a count", 1, 0, 64),
    ],
};

const NETWORKING: Kind = Kind {
    screen: "servernetworking",
    title: "Networking",
    section: Some("network"),
    note: Some("Port and HTTPS changes need a server restart. A wrong value can lock clients out."),
    fields: &[
        Field::Text("BaseUrl", "Base URL", "e.g. /jellyfin (empty = none)"),
        Field::Number("InternalHttpPort", "HTTP port", "8096", 1, 1, 65_535),
        Field::Number("PublicHttpPort", "Public HTTP port", "8096", 1, 1, 65_535),
        Field::Toggle("EnableHttps", "Enable HTTPS"),
        Field::Toggle("RequireHttps", "Require HTTPS"),
        Field::Number("InternalHttpsPort", "HTTPS port", "8920", 1, 1, 65_535),
        Field::Number("PublicHttpsPort", "Public HTTPS port", "8920", 1, 1, 65_535),
        Field::Text("CertificatePath", "HTTPS certificate file", "Server path to a .pfx"),
        Field::Toggle("EnableRemoteAccess", "Allow remote connections"),
        Field::Toggle("AutoDiscovery", "Answer client discovery"),
        Field::Toggle("EnableUPnP", "Automatic port mapping (UPnP)"),
        Field::Toggle("EnableIPv4", "Use IPv4"),
        Field::Toggle("EnableIPv6", "Use IPv6"),
        Field::List("LocalNetworkSubnets", "LAN networks", "e.g. 10.10.0.0/24", false),
        Field::List("KnownProxies", "Known proxies", "Addresses, comma-separated", false),
    ],
};

pub fn general() -> Box<dyn Screen> {
    page(GENERAL)
}
pub fn branding() -> Box<dyn Screen> {
    page(BRANDING)
}
pub fn transcoding() -> Box<dyn Screen> {
    page(TRANSCODING)
}
pub fn resume() -> Box<dyn Screen> {
    page(RESUME)
}
pub fn streaming() -> Box<dyn Screen> {
    page(STREAMING)
}
pub fn trickplay() -> Box<dyn Screen> {
    page(TRICKPLAY)
}
pub fn networking() -> Box<dyn Screen> {
    page(NETWORKING)
}

fn page(kind: Kind) -> Box<dyn Screen> {
    Box::new(ServerPage {
        kind,
        loaded: None,
        cfg: None,
        texts: vec![],
        cache_path: None,
        busy: false,
        error: None,
        notice: None,
        started: false,
        form: FormState::new(),
    })
}

struct ServerPage {
    kind: Kind,
    /// The config as last read from the server.
    loaded: Option<Value>,
    /// The same, with the form's switches already applied; text and choices are folded in on Save.
    cfg: Option<Value>,
    texts: Vec<String>,
    cache_path: Option<String>,
    busy: bool,
    error: Option<String>,
    notice: Option<String>,
    started: bool,
    form: FormState,
}

fn read(c: &crate::api::Client, section: Option<&str>) -> Result<Value> {
    match section {
        Some(s) => c.named_configuration(s),
        None => c.system_configuration(),
    }
}

impl ServerPage {
    fn load(&mut self, cx: &Cx) {
        self.error = None;
        let (c, section) = (cx.api(), self.kind.section);
        cx.spawn("config", move || read(&c, section));
        if self.kind.fields.iter().any(|f| matches!(f, Field::CachePath(_))) {
            let c = cx.api();
            cx.spawn("info", move || c.system_info());
        }
    }

    /// Put a freshly read config into the form.
    fn adopt(&mut self, v: Value) {
        self.texts = self
            .kind
            .fields
            .iter()
            .map(|f| match f {
                Field::Text(p, ..) => cfg_str(&v, p),
                Field::Number(p, _, _, unit, ..) => number_text(&v, p, *unit),
                Field::List(p, ..) => list_text(&v, p),
                _ => String::new(),
            })
            .collect();
        let mut n = 0;
        for f in self.kind.fields {
            if let Field::Choice(p, _, opts) = f {
                let o = choice_options(&v, p, opts);
                self.form.dd[n].highlighted = o.iter().position(|(val, _)| *val == cfg_str(&v, p)).unwrap_or(0);
                n += 1;
            }
        }
        self.loaded = Some(v.clone());
        self.cfg = Some(v);
    }

    /// The form's values folded into a copy of the config, or the first thing that can't be saved.
    fn edited(&self) -> std::result::Result<Value, String> {
        let mut cfg = self.cfg.clone().unwrap_or(Value::Null);
        let mut dd = 0;
        for (i, f) in self.kind.fields.iter().enumerate() {
            match f {
                Field::Text(p, ..) => set_path(&mut cfg, p, Value::String(self.texts[i].trim().to_string())),
                Field::Number(p, label, _, unit, min, max) => {
                    let t = self.texts[i].trim();
                    if t.is_empty() && get_path(&cfg, p).is_none() {
                        continue;
                    }
                    let n = parse_number(t, *unit, *min, *max)
                        .ok_or_else(|| format!("{label}: enter a number from {min} to {max}."))?;
                    set_path(&mut cfg, p, Value::from(n));
                }
                Field::List(p, label, _, numbers) => {
                    let l = parse_list(&self.texts[i], *numbers)
                        .ok_or_else(|| format!("{label}: enter whole numbers separated by commas."))?;
                    set_path(&mut cfg, p, l);
                }
                Field::Choice(p, _, opts) => {
                    let o = choice_options(&cfg, p, opts);
                    if let Some((val, _)) = o.get(self.form.dd[dd].highlighted) {
                        set_path(&mut cfg, p, Value::String(val.clone()));
                    }
                    dd += 1;
                }
                Field::Toggle(..) | Field::InList(..) | Field::CachePath(_) => {}
            }
        }
        Ok(cfg)
    }

    fn save(&mut self, cx: &Cx) {
        if self.busy {
            return;
        }
        let Some(loaded) = self.loaded.clone() else { return };
        self.notice = None;
        let edited = match self.edited() {
            Ok(e) => e,
            Err(m) => {
                self.error = Some(m);
                return;
            }
        };
        self.error = None;
        let ch = changes(&loaded, &edited, self.kind.fields);
        if ch.is_empty() {
            self.notice = Some("Nothing to save.".into());
            return;
        }
        self.busy = true;
        let (c, section) = (cx.api(), self.kind.section);
        cx.spawn("save", move || -> Result<Value> {
            let mut fresh = read(&c, section)?;
            apply(&mut fresh, &ch);
            match section {
                Some(s) => c.set_named_configuration(s, &fresh)?,
                None => c.set_system_configuration(&fresh)?,
            }
            Ok(fresh)
        });
    }
}

impl Screen for ServerPage {
    fn name(&self) -> &'static str {
        self.kind.screen
    }

    fn on_key(&mut self, _cx: &mut Cx, key: Key) -> bool {
        let (handled, pick) = self.form.key(key);
        if let Some((w, i)) = pick {
            self.form.dd[w].highlighted = i;
        }
        handled
    }

    fn on_done(&mut self, cx: &mut Cx, tag: &'static str, payload: Box<dyn std::any::Any + Send>) {
        match tag {
            "config" => {
                let Ok(r) = payload.downcast::<Result<Value>>() else { return };
                match *r {
                    Ok(v) => self.adopt(v),
                    Err(e) => self.error = fail(cx, e),
                }
            }
            "info" => {
                let Ok(r) = payload.downcast::<Result<SystemInfo>>() else { return };
                match *r {
                    Ok(i) => self.cache_path = i.cache_path,
                    Err(ApiError::Unauthorized) => {
                        cx.signed_out();
                    }
                    Err(_) => {}
                }
            }
            "save" => {
                self.busy = false;
                let Ok(r) = payload.downcast::<Result<Value>>() else { return };
                match *r {
                    Ok(v) => {
                        self.adopt(v);
                        self.notice = Some("Saved.".into());
                    }
                    Err(e) => self.error = fail(cx, e),
                }
            }
            _ => {}
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        let t = cx.theme;
        if !self.started {
            self.started = true;
            self.load(cx);
        }
        let moving = ui.input(|i| i.pointer.is_moving());
        ui.add_space(t.margin);
        parts::title(ui, t, self.kind.title);
        ui.add_space(t.gap / 2.0);
        let mut ctl = self.form.ctl(moving, true);
        let mut save = false;
        let mut retry = false;
        if let Some(e) = self.error.clone() {
            parts::error_line(ui, t, &e);
            if self.cfg.is_none() {
                retry = ctl.button(ui, t, "Retry", false);
            }
        }
        if let Some(n) = &self.notice {
            parts::muted(ui, t, n);
        }
        if let Some(n) = self.kind.note {
            parts::muted(ui, t, n);
        }
        if self.cfg.is_none() {
            if self.error.is_none() {
                skeleton_rows(ui, t, 4);
            }
        } else {
            let kind = self.kind;
            egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| {
                let mut dd = 0;
                for (i, f) in kind.fields.iter().enumerate() {
                    ui.add_space(t.gap / 3.0);
                    match f {
                        Field::Toggle(p, label) => {
                            let on = self.cfg.as_ref().is_some_and(|c| cfg_bool(c, p));
                            if ctl.toggle(ui, t, label, on) {
                                if let Some(c) = self.cfg.as_mut() {
                                    set_path(c, p, Value::Bool(!on));
                                }
                            }
                        }
                        Field::InList(p, label, entry) => {
                            let on = self.cfg.as_ref().is_some_and(|c| in_list(c, p, entry));
                            if ctl.toggle(ui, t, label, on) {
                                if let Some(c) = self.cfg.as_mut() {
                                    toggle_in_list(c, p, entry);
                                }
                            }
                        }
                        Field::Text(_, label, hint)
                        | Field::Number(_, label, hint, ..)
                        | Field::List(_, label, hint, _) => {
                            parts::muted(ui, t, label);
                            ctl.field(ui, t, &mut self.texts[i], hint, false);
                        }
                        Field::Choice(p, label, opts) => {
                            parts::muted(ui, t, label);
                            let cfg = self.cfg.clone().unwrap_or(Value::Null);
                            let labels: Vec<String> =
                                choice_options(&cfg, p, opts).into_iter().map(|(_, l)| l).collect();
                            if let Some(pick) = ctl.dropdown(ui, t, &mut self.form.dd[dd], dd, "", &labels) {
                                self.form.dd[dd].highlighted = pick;
                            }
                            dd += 1;
                        }
                        Field::CachePath(label) => {
                            parts::muted(ui, t, label);
                            parts::muted(ui, t, self.cache_path.as_deref().unwrap_or("…"));
                        }
                    }
                }
                ui.add_space(t.gap);
                let label = if self.busy { "Saving…" } else { "Save" };
                save = ctl.button(ui, t, label, true);
            });
        }
        self.form.finish(ctl);
        if retry {
            self.load(cx);
        }
        if save {
            self.save(cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn paths_get_and_set() {
        let mut v = json!({"A": {"B": 1}, "C": "x"});
        assert_eq!(get_path(&v, "A.B"), Some(&json!(1)));
        assert_eq!(get_path(&v, "A.Z"), None);
        set_path(&mut v, "A.B", json!(2));
        set_path(&mut v, "N.M.K", json!(true));
        set_path(&mut v, "D", json!("d"));
        assert_eq!(v, json!({"A": {"B": 2}, "C": "x", "N": {"M": {"K": true}}, "D": "d"}));
    }

    #[test]
    fn numbers_with_units() {
        assert_eq!(number_text(&json!({"R": 20_000_000}), "R", 1_000_000), "20");
        assert_eq!(number_text(&json!({"R": 2_500_000}), "R", 1_000_000), "2.5");
        assert_eq!(number_text(&json!({"R": 0}), "R", 1_000_000), "0");
        assert_eq!(number_text(&json!({}), "R", 1), "");
        assert_eq!(parse_number("20", 1_000_000, 0, 1000), Some(20_000_000));
        assert_eq!(parse_number(" 2.5 ", 1_000_000, 0, 1000), Some(2_500_000));
        assert_eq!(parse_number("-1", 1, -1, 256), Some(-1));
        assert_eq!(parse_number("101", 1, 0, 100), None);
        assert_eq!(parse_number("abc", 1, 0, 100), None);
        assert_eq!(parse_number("", 1, 0, 100), None);
    }

    #[test]
    fn lists_round_trip() {
        let v = json!({"S": ["10.10.0.0/24", "10.0.0.0/8"], "W": [320, 640], "E": []});
        assert_eq!(list_text(&v, "S"), "10.10.0.0/24, 10.0.0.0/8");
        assert_eq!(list_text(&v, "W"), "320, 640");
        assert_eq!(list_text(&v, "E"), "");
        assert_eq!(parse_list("a, b ,, c", false), Some(json!(["a", "b", "c"])));
        assert_eq!(parse_list("320, 640", true), Some(json!([320, 640])));
        assert_eq!(parse_list("320, x", true), None);
        assert_eq!(parse_list("", true), Some(json!([])));
    }

    #[test]
    fn list_switches() {
        let mut v = json!({"C": ["h264", "vc1"]});
        assert!(in_list(&v, "C", "h264") && !in_list(&v, "C", "hevc"));
        toggle_in_list(&mut v, "C", "hevc");
        toggle_in_list(&mut v, "C", "h264");
        assert_eq!(v["C"], json!(["vc1", "hevc"]));
        let mut empty = json!({});
        toggle_in_list(&mut empty, "C", "av1");
        assert_eq!(empty["C"], json!(["av1"]));
    }

    #[test]
    fn choices_keep_an_unknown_stored_value() {
        let v = json!({"H": "weird"});
        let o = choice_options(&v, "H", HW_ACCEL);
        assert_eq!(o.len(), HW_ACCEL.len() + 1);
        assert_eq!(o.last().unwrap().0, "weird");
        assert_eq!(choice_options(&json!({"H": "nvenc"}), "H", HW_ACCEL).len(), HW_ACCEL.len());
        assert_eq!(choice_options(&json!({}), "H", HW_ACCEL).len(), HW_ACCEL.len());
    }

    #[test]
    fn only_edited_fields_are_saved_and_the_rest_is_kept() {
        let fields = RESUME.fields;
        let loaded = json!({"MinResumePct": 5, "MaxResumePct": 90, "MinResumeDurationSeconds": 1, "Other": [1]});
        let mut edited = loaded.clone();
        assert!(changes(&loaded, &edited, fields).is_empty());
        set_path(&mut edited, "MaxResumePct", json!(95));
        let ch = changes(&loaded, &edited, fields);
        assert_eq!(ch, vec![("MaxResumePct", json!(95))]);
        // The server's copy moved on meanwhile: only the edited field is replaced.
        let mut fresh = json!({"MinResumePct": 7, "MaxResumePct": 90, "MinResumeDurationSeconds": 1, "Other": [1, 2]});
        apply(&mut fresh, &ch);
        assert_eq!(
            fresh,
            json!({"MinResumePct": 7, "MaxResumePct": 95, "MinResumeDurationSeconds": 1, "Other": [1, 2]})
        );
    }

    #[test]
    fn empty_text_that_was_never_set_is_not_a_change() {
        let fields = BRANDING.fields;
        let loaded = json!({"SplashscreenEnabled": false});
        let mut edited = loaded.clone();
        set_path(&mut edited, "LoginDisclaimer", json!(""));
        set_path(&mut edited, "CustomCss", json!(""));
        assert!(changes(&loaded, &edited, fields).is_empty());
        set_path(&mut edited, "LoginDisclaimer", json!("Hello"));
        assert_eq!(changes(&loaded, &edited, fields), vec![("LoginDisclaimer", json!("Hello"))]);
        // Clearing a set text is a change.
        let loaded2 = json!({"LoginDisclaimer": "Hello"});
        let edited2 = json!({"LoginDisclaimer": ""});
        assert_eq!(changes(&loaded2, &edited2, fields), vec![("LoginDisclaimer", json!(""))]);
    }

    #[test]
    fn nested_trickplay_fields() {
        let loaded = json!({"TrickplayOptions": {"Interval": 10000, "WidthResolutions": [320]}});
        let mut edited = loaded.clone();
        set_path(&mut edited, "TrickplayOptions.WidthResolutions", json!([320, 640]));
        assert_eq!(
            changes(&loaded, &edited, TRICKPLAY.fields),
            vec![("TrickplayOptions.WidthResolutions", json!([320, 640]))]
        );
    }

    #[test]
    fn every_page_names_a_unique_screen() {
        let all = [GENERAL, BRANDING, TRANSCODING, RESUME, STREAMING, TRICKPLAY, NETWORKING];
        let mut names: Vec<_> = all.iter().map(|k| k.screen).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), all.len());
        for k in all {
            let choices = k.fields.iter().filter(|f| matches!(f, Field::Choice(..))).count();
            assert!(choices <= 6, "{} has too many dropdowns", k.screen);
        }
    }
}
