//! Jellyfin client core. No window, no player, no browser: the shell (desktop or TV) gives it a GL context
//! through egui and implements the platform traits.
pub mod api;
pub mod app;
pub mod discovery;
pub mod images;
pub mod nav;
pub mod order;
pub mod platform;
pub mod playreq;
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
