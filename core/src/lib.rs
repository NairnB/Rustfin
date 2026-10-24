//! Jellyfin client core. No window, no player, no browser: the shell (desktop or TV) gives it a GL context
//! through egui and implements the platform traits.
pub mod api;
pub mod app;
pub mod discovery;
pub mod images;
pub mod keyhold;
pub mod nav;
pub mod order;
pub mod platform;
pub mod playreq;
pub mod queue;
pub mod repeat;
pub mod sections;
pub mod settings;
pub mod show;
pub mod story;
pub mod tags;
pub mod tasks;
pub mod theme;
pub mod tmdb;
pub mod ui;
#[cfg(target_arch = "wasm32")]
pub mod webhttp;

/// Clock that also works in the browser (std's panics on wasm32; TV web route).
pub(crate) mod time {
    #[cfg(not(target_arch = "wasm32"))]
    pub use std::time::{Instant, SystemTime, UNIX_EPOCH};
    #[cfg(target_arch = "wasm32")]
    pub use web_time::{Instant, SystemTime, UNIX_EPOCH};
}
