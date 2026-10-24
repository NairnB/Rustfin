//! FIX-2 #8 (FRICTION-3): Info on a poster opens this small menu for the item — Play, Details, Mark watched,
//! Favourite, Poster Flow — the tvOS long-press menu. Before, Info jumped straight into Poster Flow, whose card has
//! no watched toggle, and testers could not find their way back.
//!
//! The entries and the key handling are pure (unit-tested); the screen that owns the menu draws it with `draw` and
//! acts on the returned `MenuAct` (Play / Details / the server toggles are done here by `run`).
use super::widgets;
use crate::api::{ApiError, Item, ItemKind, Result};
use crate::app::Cx;
use crate::platform::Key;
use crate::theme::Weight;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAct {
    Play,
    Details,
    ToggleWatched,
    ToggleFavourite,
    PosterFlow,
    /// PP-1: open the poster picker for this item.
    ChangePoster,
}

/// The menu's rows for `item`, in order. Play only for things that play on their own (films, episodes, videos);
/// the watched / favourite labels say what pressing them will do. The poster picker is always offered (a
/// personal pick needs no rights); its label says so for non-admins (PP-1).
pub fn entries(item: &Item, admin: bool) -> Vec<(MenuAct, String)> {
    let ud = item.user_data.clone().unwrap_or_default();
    let mut v = vec![];
    if matches!(item.kind, ItemKind::Movie | ItemKind::Episode | ItemKind::Other) {
        v.push((MenuAct::Play, "Play".to_string()));
    }
    v.push((MenuAct::Details, "Details".to_string()));
    v.push((MenuAct::ToggleWatched, if ud.played { "Mark unwatched" } else { "Mark watched" }.to_string()));
    v.push((
        MenuAct::ToggleFavourite,
        if ud.is_favorite { "Remove from Favourites" } else { "Add to Favourites" }.to_string(),
    ));
    v.push((
        MenuAct::ChangePoster,
        if admin { "Change poster…".to_string() } else { "Change poster (just for you)…".to_string() },
    ));
    // Poster Flow stays the last row (tours and fleet routes reach it with Down to the end).
    v.push((MenuAct::PosterFlow, "Poster Flow".to_string()));
    v
}

/// What a key does to the open menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuKey {
    /// Focus moved (or nothing happened); the menu stays.
    Stay,
    Close,
    Pick(MenuAct),
}

pub struct CardMenu {
    pub item: Item,
    pub focus: usize,
    pub admin: bool,
}

impl CardMenu {
    pub fn new(item: Item, admin: bool) -> CardMenu {
        CardMenu { item, focus: 0, admin }
    }

    /// Up/Down move (no wrap), Enter picks, Back / Info close. Every key is the menu's while it is open.
    pub fn key(&mut self, key: Key) -> MenuKey {
        let n = entries(&self.item, self.admin).len();
        match key {
            Key::Up => self.focus = self.focus.saturating_sub(1),
            Key::Down => self.focus = (self.focus + 1).min(n.saturating_sub(1)),
            Key::Enter => {
                return entries(&self.item, self.admin)
                    .get(self.focus)
                    .map_or(MenuKey::Close, |(a, _)| MenuKey::Pick(*a))
            }
            Key::Back | Key::Info => return MenuKey::Close,
            _ => {}
        }
        MenuKey::Stay
    }

    /// A centred translucent card: the item's name, then one pill per entry (focused = white plate). Returns a
    /// clicked entry; hover moves the focus (mouse and remote look the same).
    pub fn draw(&mut self, ui: &mut egui::Ui, cx: &Cx) -> Option<MenuAct> {
        let t = cx.theme;
        let screen = ui.ctx().screen_rect();
        ui.painter().rect_filled(screen, 0.0, t.dim_scrim);
        let rows = entries(&self.item, self.admin);
        let mut picked = None;
        let mut hovered = None;
        let w = t.dropdown_w + 2.0 * t.menu_pad;
        egui::Area::new(egui::Id::new("card-menu"))
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                egui::Frame::NONE
                    .fill(t.menu_fill)
                    .corner_radius(t.dialog_radius)
                    .inner_margin(t.dialog_pad)
                    .shadow(t.shadow(1.0))
                    .show(ui, |ui| {
                        ui.set_width(w);
                        ui.vertical_centered(|ui| {
                            ui.spacing_mut().item_spacing.y = t.gap / 4.0;
                            ui.label(
                                egui::RichText::new(&self.item.name)
                                    .font(t.font_w(t.heading_size, Weight::Bold))
                                    .color(t.text),
                            );
                            ui.add_space(t.gap / 4.0);
                            for (i, (act, label)) in rows.iter().enumerate() {
                                let r = widgets::pill_button_width(ui, t, label, self.focus == i, false, w);
                                if r.hovered() {
                                    hovered = Some(i);
                                }
                                if r.clicked() {
                                    picked = Some(*act);
                                }
                            }
                        });
                    });
            });
        if let Some(i) = hovered {
            self.focus = i;
        }
        picked
    }
}

/// Does `act` for `item`: Play and Details open their screens; the toggles go to the server (tag "cardmenu",
/// payload `Result<()>`) and return the item as it will be, for the caller to show at once.
/// PosterFlow is the caller's (it knows the list).
pub fn run(cx: &mut Cx, item: &Item, act: MenuAct, place: Option<String>) -> Option<Item> {
    match act {
        MenuAct::Play => {
            cx.push(super::player::screen(item.clone(), super::player::PlayOptions::default()));
            None
        }
        MenuAct::Details => {
            cx.push(super::item::screen_in(item.clone(), place));
            None
        }
        MenuAct::ToggleWatched | MenuAct::ToggleFavourite => {
            let mut next = item.clone();
            let ud = next.user_data.get_or_insert_with(Default::default);
            let client = cx.api();
            let id = item.id.clone();
            if act == MenuAct::ToggleWatched {
                ud.played = !ud.played;
                let v = ud.played;
                cx.spawn("cardmenu", move || client.set_played(&id, v));
            } else {
                ud.is_favorite = !ud.is_favorite;
                let v = ud.is_favorite;
                cx.spawn("cardmenu", move || client.set_favorite(&id, v));
            }
            Some(next)
        }
        MenuAct::PosterFlow => None,
        // PP-1: the caller opens the picker (it knows the admin flag and the place).
        MenuAct::ChangePoster => None,
    }
}

/// The caller's `on_done` for tag "cardmenu": signed out on Unauthorized, else an error line (or None).
pub fn done(cx: &mut Cx, payload: Box<dyn std::any::Any + Send>) -> Option<String> {
    let Ok(r) = payload.downcast::<Result<()>>() else { return None };
    match *r {
        Ok(()) => None,
        Err(ApiError::Unauthorized) => {
            cx.signed_out();
            None
        }
        Err(e) => Some(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::UserData;

    fn film(played: bool, fav: bool) -> Item {
        Item {
            name: "Amber Tide".into(),
            kind: ItemKind::Movie,
            user_data: Some(UserData { played, is_favorite: fav, ..Default::default() }),
            ..Default::default()
        }
    }

    #[test]
    fn film_menu_plays_and_says_what_the_toggles_do() {
        let e = entries(&film(false, false), true);
        let labels: Vec<&str> = e.iter().map(|(_, l)| l.as_str()).collect();
        assert_eq!(labels, ["Play", "Details", "Mark watched", "Add to Favourites", "Change poster…", "Poster Flow"]);
        let e = entries(&film(true, true), false);
        assert_eq!(e[2].1, "Mark unwatched");
        assert_eq!(e[3].1, "Remove from Favourites");
        assert_eq!(e[4].1, "Change poster (just for you)…");
    }

    #[test]
    fn a_show_has_no_play_row() {
        let show = Item { kind: ItemKind::Series, ..Default::default() };
        assert_eq!(entries(&show, false)[0].0, MenuAct::Details);
    }

    #[test]
    fn keys_move_pick_and_close() {
        let mut m = CardMenu::new(film(false, false), false);
        assert_eq!(m.key(Key::Up), MenuKey::Stay);
        assert_eq!(m.focus, 0);
        m.key(Key::Down);
        m.key(Key::Down);
        assert_eq!(m.key(Key::Enter), MenuKey::Pick(MenuAct::ToggleWatched));
        for _ in 0..10 {
            m.key(Key::Down);
        }
        assert_eq!(m.focus, 5, "no wrap past the last row");
        assert_eq!(m.key(Key::Back), MenuKey::Close);
        assert_eq!(m.key(Key::Info), MenuKey::Close);
    }
}
