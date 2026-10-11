//! RR8 / P-USER-2: the screensaver. After the idle time on a screen that is not the player, a full-screen slideshow
//! of library backdrops (or the app name drifting about); any key or mouse move ends it. The app owns one `Overlay`
//! and draws it instead of the top screen while it is up.
use crate::api::{Client, ImageKind, Item};
use crate::theme::Theme;
use crate::ui::imageload::Images;
use egui::{pos2, Align2, Rect};

pub use crate::settings::Screensaver;

/// How many backdrops one run loads.
const SLIDES: usize = 30;
/// Logo drift speed, as a fraction of the screen per second.
const DRIFT: f32 = 0.06;

/// Whether the screensaver should start: it is on, the app has been idle for `minutes`, and the top screen is not the
/// player. Times are app seconds.
pub fn idle_due(mode: Screensaver, minutes: u32, last_input: f64, now: f64, over_player: bool) -> bool {
    mode != Screensaver::None && !over_player && now - last_input >= f64::from(minutes.max(1)) * 60.0
}

pub struct Overlay {
    mode: Screensaver,
    pub items: Vec<Item>,
    pub loaded: bool,
    started: f64,
}

impl Overlay {
    pub fn new(mode: Screensaver, now: f64) -> Overlay {
        Overlay { mode, items: vec![], loaded: mode != Screensaver::Backdrops, started: now }
    }

    pub fn wants_items(&self) -> bool {
        self.mode == Screensaver::Backdrops && !self.loaded
    }

    pub fn set_items(&mut self, items: Vec<Item>) {
        self.items = items;
        self.loaded = true;
    }

    pub fn slides() -> usize {
        SLIDES
    }

    /// Draws the whole frame: black, then the current backdrop or the drifting name.
    pub fn ui(&self, ui: &mut egui::Ui, t: &Theme, client: Option<&Client>, images: &mut Images, now: f64) {
        let rect = ui.max_rect();
        let painter = ui.painter();
        painter.rect_filled(rect, 0.0, t.bg);
        let elapsed = (now - self.started) as f32;
        match self.mode {
            Screensaver::Backdrops => {
                let Some(client) = client else { return };
                if self.items.is_empty() {
                    return;
                }
                let n = (elapsed / t.slide_secs.max(1.0)) as usize;
                let w = crate::api::image_width_bucket(rect.width(), ui.ctx().pixels_per_point());
                // Keep the next slide warm so the switch is instant.
                for k in [n + 1, n] {
                    let item = &self.items[k % self.items.len()];
                    if let Some(url) = client.image_url(item, ImageKind::Backdrop, w) {
                        if let Some(tex) = images.get(&url) {
                            if k == n {
                                let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                                painter.image(tex, rect, uv, egui::Color32::WHITE);
                            }
                        }
                    }
                }
            }
            Screensaver::Logo => {
                let (x, y) = (bounce(elapsed * DRIFT), bounce(elapsed * DRIFT * 0.7 + 0.3));
                let at =
                    pos2(rect.left() + rect.width() * (0.15 + 0.7 * x), rect.top() + rect.height() * (0.15 + 0.7 * y));
                painter.text(at, Align2::CENTER_CENTER, "Rustfin", t.font(t.title_size * 2.0), t.text);
            }
            Screensaver::None => {}
        }
    }
}

/// A triangle wave 0..1..0 with period 2.
fn bounce(x: f32) -> f32 {
    let p = x.rem_euclid(2.0);
    if p > 1.0 {
        2.0 - p
    } else {
        p
    }
}
