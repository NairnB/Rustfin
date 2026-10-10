//! Screens (SPEC §8). Each module exposes `screen(...) -> Box<dyn Screen>`.
pub mod admin;
pub mod admin_actions;
pub mod collection;
pub mod connect;
pub mod favourites;
pub mod gallery;
pub mod home;
pub mod imageload;
pub mod item;
pub mod library;
pub mod parts;
pub mod person;
pub mod player;
pub mod playlists;
pub mod quickconnect;
pub mod search;
pub mod settings_screen;
pub mod show_page;
pub mod signin;
pub mod widgets;

use crate::app::{Cx, Screen};
use crate::platform::Key;

/// Placeholder for screens not built yet (packets S1–S3, P2 replace these).
pub struct Todo(pub &'static str);
impl Screen for Todo {
    fn name(&self) -> &'static str {
        self.0
    }
    fn on_key(&mut self, _cx: &mut Cx, _key: Key) -> bool {
        false
    }
    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        parts::title(ui, cx.theme, self.0);
        parts::muted(ui, cx.theme, "Not built yet.");
    }
}
