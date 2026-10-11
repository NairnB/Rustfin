//! NAV (BRIEF-desktop-nav): the desktop app shell — a left sidebar (Home · Search · each library · Collections ·
//! Playlists · Favourites, the user and Server admin at the bottom; collapsible to icons) and a toolbar with
//! Back / Forward arrows and a clickable breadcrumb. Drawn by `App` around signed-in viewer screens when
//! `AppOptions::desktop` is on; the TV build keeps remote-first navigation. Mouse first: every row is a click
//! target with a hover highlight; the keyboard keeps working inside the screen (Back, Forward, Cmd/Ctrl+F).
use crate::api::Item;
use crate::app::Screen;
use crate::theme::Theme;
use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Shape, Stroke, Ui};

/// A top-level place in the sidebar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Place {
    Home,
    Search,
    /// A server library, by view id.
    View(String),
    Collections,
    Playlists,
    Favourites,
    Settings,
    Admin,
}

/// What the user clicked in the shell this frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Back,
    Forward,
    /// Breadcrumb part of the `i`-th screen above Home.
    Crumb(usize),
    Open(Place),
    Collapse,
    SwitchUser,
    SignOut,
}

/// Everything the shell shows, read from the app.
pub struct View<'a> {
    pub views: &'a [Item],
    pub user: &'a str,
    pub admin: bool,
    pub collapsed: bool,
    /// The highlighted place (None: e.g. the user menu's Settings is open).
    pub place: Option<Place>,
    pub can_back: bool,
    pub can_forward: bool,
    /// Breadcrumb parts per screen above Home.
    pub crumbs: &'a [Vec<String>],
}

/// Libraries the sidebar lists: everything but collections and playlists (they have their own rows).
pub fn sidebar_views(views: &[Item]) -> impl Iterator<Item = &Item> {
    views.iter().filter(|v| !matches!(v.collection_type.as_deref(), Some("boxsets") | Some("playlists")))
}

/// The breadcrumb part(s) of a screen: its own `crumbs()`, else a label for its name.
pub fn crumbs_of(s: &dyn Screen) -> Vec<String> {
    let c = s.crumbs();
    if !c.is_empty() {
        return c;
    }
    let label = match s.name() {
        "home" => "Home",
        "search" => "Search",
        "favourites" => "Favourites",
        "playlists" => "Playlists",
        "collections" => "Collections",
        "settings" => "Settings",
        "admin" => "Server admin",
        "gallery" => "Gallery",
        other => return vec![capitalise(other)],
    };
    vec![label.to_string()]
}

fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

/// The sidebar place for the screen just above Home (`(name, crumbs)`); anything opened from Home's rows stays
/// under Home.
pub fn place_of(second: Option<(&str, &[String])>, views: &[Item]) -> Option<Place> {
    let Some((name, crumbs)) = second else { return Some(Place::Home) };
    Some(match name {
        "search" => Place::Search,
        "collections" => Place::Collections,
        "playlists" => Place::Playlists,
        "favourites" => Place::Favourites,
        "settings" => return None,
        "admin" => Place::Admin,
        "library" => match crumbs.first().and_then(|c| views.iter().find(|v| &v.name == c)) {
            Some(v) => Place::View(v.id.clone()),
            None => Place::Home,
        },
        _ => Place::Home,
    })
}

/// The screen a place opens (None = Home: the bottom of the stack).
pub fn open(place: Place, views: &[Item]) -> Option<Box<dyn Screen>> {
    match place {
        Place::Home => None,
        Place::Search => Some(super::search::screen()),
        Place::View(id) => views.iter().find(|v| v.id == id).map(|v| super::library::screen(v.clone())),
        Place::Collections => Some(super::collection::list()),
        Place::Playlists => Some(super::playlists::list()),
        Place::Favourites => Some(super::favourites::screen()),
        Place::Settings => Some(super::settings_screen::screen()),
        Place::Admin => Some(super::admin::screen()),
    }
}

/// NAV: focus marks (rings, plates) show only after the keyboard was used, not after a mouse click. `App`
/// stores the mode each frame; parts read it here.
pub fn keyboard_focus(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp(egui::Id::new(KBD_ID))).unwrap_or(true)
}
pub(crate) fn set_keyboard_focus(ctx: &egui::Context, on: bool) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(KBD_ID), on));
}
const KBD_ID: &str = "nav-keyboard-focus";

/// Draws the sidebar and the toolbar as egui panels (call before the screen's `CentralPanel`).
pub fn draw(ctx: &egui::Context, t: &Theme, v: &View) -> Option<Action> {
    let mut out = None;
    let w = if v.collapsed { t.sidebar_rail_w } else { t.sidebar_w };
    egui::SidePanel::left("nav-sidebar")
        .exact_width(w)
        .resizable(false)
        .show_separator_line(false)
        .frame(egui::Frame::NONE.fill(t.sidebar_fill))
        .show(ctx, |ui| out = sidebar(ui, t, v));
    egui::TopBottomPanel::top("nav-toolbar")
        .exact_height(t.toolbar_h)
        .show_separator_line(false)
        .frame(egui::Frame::NONE.fill(t.bg))
        .show(ctx, |ui| {
            if let Some(a) = toolbar(ui, t, v) {
                out = Some(a);
            }
        });
    out
}

#[derive(Clone, Copy)]
enum Glyph {
    Sidebar,
    Home,
    Search,
    Film,
    Tv,
    Music,
    Folder,
    Stack,
    List,
    Heart,
    Server,
    Back,
    Forward,
}

fn view_glyph(v: &Item) -> Glyph {
    match v.collection_type.as_deref() {
        Some("movies") => Glyph::Film,
        Some("tvshows") => Glyph::Tv,
        Some("music") => Glyph::Music,
        _ => Glyph::Folder,
    }
}

fn sidebar(ui: &mut Ui, t: &Theme, v: &View) -> Option<Action> {
    let mut out = None;
    let full = ui.max_rect();
    let pad = t.sidebar_pad;
    let row_w = full.width() - 2.0 * pad;
    let mut y = full.top() + (t.toolbar_h - t.sidebar_item_h) / 2.0;

    // Collapse toggle, then the app name.
    let toggle = Rect::from_min_size(pos2(full.left() + pad, y), vec2(t.sidebar_item_h, t.sidebar_item_h));
    let r = ui.interact(toggle, ui.id().with("nav-collapse"), Sense::click());
    if r.hovered() {
        ui.painter().rect_filled(toggle, t.radius, t.sidebar_hover);
    }
    glyph(ui, t, Glyph::Sidebar, toggle.center(), if r.hovered() { t.text } else { t.muted });
    if r.clicked() {
        out = Some(Action::Collapse);
    }
    let r = r.on_hover_text(if v.collapsed { "Show sidebar" } else { "Hide sidebar" });
    let _ = r;
    if !v.collapsed {
        ui.painter().text(
            pos2(toggle.right() + pad / 2.0, toggle.center().y),
            Align2::LEFT_CENTER,
            "Rustfin",
            t.font(t.heading_size),
            t.text,
        );
    }
    y = full.top() + t.toolbar_h + pad / 2.0;

    let mut item = |ui: &mut Ui, y: &mut f32, g: Glyph, label: &str, place: Place, id: &str| {
        let rect = Rect::from_min_size(pos2(full.left() + pad, *y), vec2(row_w, t.sidebar_item_h));
        *y += t.sidebar_item_h + t.sidebar_gap;
        if row(ui, t, rect, g, label, v.place.as_ref() == Some(&place), v.collapsed, id) {
            out = Some(Action::Open(place));
        }
    };
    item(ui, &mut y, Glyph::Home, "Home", Place::Home, "home");
    item(ui, &mut y, Glyph::Search, "Search", Place::Search, "search");

    // Libraries from the server.
    y += t.sidebar_gap * 2.0;
    if !v.collapsed {
        ui.painter().text(
            pos2(full.left() + pad + t.sidebar_text_x / 4.0, y),
            Align2::LEFT_TOP,
            "Library",
            t.font(t.small_size),
            t.muted,
        );
        y += t.small_size + t.sidebar_gap * 2.0;
    }
    for lib in sidebar_views(v.views) {
        let id = format!("view-{}", lib.id);
        item(ui, &mut y, view_glyph(lib), &lib.name, Place::View(lib.id.clone()), &id);
    }
    item(ui, &mut y, Glyph::Stack, "Collections", Place::Collections, "collections");
    item(ui, &mut y, Glyph::List, "Playlists", Place::Playlists, "playlists");
    item(ui, &mut y, Glyph::Heart, "Favourites", Place::Favourites, "favourites");

    // Bottom: Server admin (admins), then the user with its menu.
    let mut yb = full.bottom() - pad - t.sidebar_user_h;
    let user_rect = Rect::from_min_size(pos2(full.left() + pad, yb), vec2(row_w, t.sidebar_user_h));
    if v.admin {
        yb -= t.sidebar_item_h + t.sidebar_gap;
        let rect = Rect::from_min_size(pos2(full.left() + pad, yb), vec2(row_w, t.sidebar_item_h));
        if row(ui, t, rect, Glyph::Server, "Server admin", v.place == Some(Place::Admin), v.collapsed, "admin") {
            out = Some(Action::Open(Place::Admin));
        }
    }
    if let Some(a) = user_row(ui, t, user_rect, v) {
        out = Some(a);
    }
    out
}

/// One sidebar row: icon, label (hidden when collapsed), hover and selected fills. Returns clicked.
#[allow(clippy::too_many_arguments)]
fn row(ui: &mut Ui, t: &Theme, rect: Rect, g: Glyph, label: &str, selected: bool, collapsed: bool, id: &str) -> bool {
    let r = ui.interact(rect, ui.id().with(("nav-row", id)), Sense::click());
    let fill = if selected {
        t.sidebar_selected
    } else if r.hovered() {
        t.sidebar_hover
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, t.radius, fill);
    let ink = if selected || r.hovered() { t.text } else { t.sidebar_ink };
    let icon_c = pos2(rect.left() + t.sidebar_item_h / 2.0, rect.center().y);
    glyph(ui, t, g, icon_c, ink);
    if !collapsed {
        let max_w = rect.right() - (rect.left() + t.sidebar_text_x) - t.sidebar_pad / 2.0;
        let job = egui::text::LayoutJob::simple_singleline(label.to_string(), t.font(t.body_size), ink);
        let mut job = job;
        job.wrap = egui::text::TextWrapping::truncate_at_width(max_w);
        let galley = ui.fonts(|f| f.layout_job(job));
        let at = pos2(rect.left() + t.sidebar_text_x, rect.center().y - galley.size().y / 2.0);
        ui.painter().galley(at, galley, ink);
    } else {
        let _ = r.clone().on_hover_text(label);
    }
    r.clicked()
}

/// The user at the bottom: initials avatar + name. Click opens a small menu above it (Settings, Switch user,
/// Sign out).
fn user_row(ui: &mut Ui, t: &Theme, rect: Rect, v: &View) -> Option<Action> {
    let menu_id = ui.id().with("nav-user-menu");
    let mut open: bool = ui.ctx().data(|d| d.get_temp(menu_id)).unwrap_or(false);
    let r = ui.interact(rect, ui.id().with("nav-user"), Sense::click());
    let fill = if open || r.hovered() { t.sidebar_hover } else { Color32::TRANSPARENT };
    ui.painter().rect_filled(rect, t.radius, fill);
    let d = t.sidebar_avatar;
    let c = pos2(rect.left() + t.sidebar_item_h / 2.0, rect.center().y);
    ui.painter().circle_filled(c, d / 2.0, t.surface_hi);
    let initials: String = v.user.split_whitespace().filter_map(|w| w.chars().next()).take(2).collect();
    let initials = if initials.is_empty() { "?".to_string() } else { initials.to_uppercase() };
    ui.painter().text(c, Align2::CENTER_CENTER, initials, t.font(t.small_size), t.text);
    if !v.collapsed {
        ui.painter().text(
            pos2(rect.left() + t.sidebar_text_x, rect.center().y),
            Align2::LEFT_CENTER,
            v.user,
            t.font(t.body_size),
            t.text,
        );
    }
    if r.clicked() {
        open = !open;
    }
    let mut out = None;
    if open {
        const ITEMS: [&str; 3] = ["Settings", "Switch user", "Sign out"];
        let w = t.sidebar_menu_w;
        let h = ITEMS.len() as f32 * t.sidebar_item_h + t.sidebar_pad;
        let menu = Rect::from_min_size(pos2(rect.left(), rect.top() - h - t.sidebar_gap), vec2(w, h));
        let mut clicked = None;
        egui::Area::new(menu_id.with("area")).order(egui::Order::Foreground).fixed_pos(menu.min).show(ui.ctx(), |ui| {
            ui.painter().rect_filled(menu, t.radius * 1.5, t.surface_hi);
            for (i, label) in ITEMS.iter().enumerate() {
                let rr = Rect::from_min_size(
                    pos2(
                        menu.left() + t.sidebar_pad / 2.0,
                        menu.top() + t.sidebar_pad / 2.0 + i as f32 * t.sidebar_item_h,
                    ),
                    vec2(w - t.sidebar_pad, t.sidebar_item_h),
                );
                let resp = ui.interact(rr, menu_id.with(i), Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(rr, t.radius, t.sidebar_hover);
                }
                ui.painter().text(
                    pos2(rr.left() + t.sidebar_pad / 2.0, rr.center().y),
                    Align2::LEFT_CENTER,
                    *label,
                    t.font(t.body_size),
                    t.text,
                );
                if resp.clicked() {
                    clicked = Some(i);
                }
            }
            ui.allocate_rect(menu, Sense::hover());
        });
        if let Some(i) = clicked {
            open = false;
            out = Some(match i {
                0 => Action::Open(Place::Settings),
                1 => Action::SwitchUser,
                _ => Action::SignOut,
            });
        } else if !r.clicked() && ui.input(|i| i.pointer.any_click()) {
            // A click anywhere else closes the menu.
            open = false;
        }
    }
    ui.ctx().data_mut(|d| d.insert_temp(menu_id, open));
    out
}

fn toolbar(ui: &mut Ui, t: &Theme, v: &View) -> Option<Action> {
    let mut out = None;
    let full = ui.max_rect();
    let d = t.toolbar_button;
    let cy = full.center().y;
    let mut x = full.left() + t.sidebar_pad;
    for (glyph_kind, enabled, action, id) in
        [(Glyph::Back, v.can_back, Action::Back, "back"), (Glyph::Forward, v.can_forward, Action::Forward, "fwd")]
    {
        let rect = Rect::from_center_size(pos2(x + d / 2.0, cy), vec2(d, d));
        x += d + t.sidebar_gap * 2.0;
        let r =
            ui.interact(rect, ui.id().with(("nav-arrow", id)), if enabled { Sense::click() } else { Sense::hover() });
        if enabled && r.hovered() {
            ui.painter().circle_filled(rect.center(), d / 2.0, t.sidebar_hover);
        }
        let ink = if !enabled {
            t.muted.gamma_multiply(0.4)
        } else if r.hovered() {
            t.text
        } else {
            t.sidebar_ink
        };
        glyph(ui, t, glyph_kind, rect.center(), ink);
        if enabled && r.clicked() {
            out = Some(action);
        }
    }

    // Breadcrumb: earlier parts muted and clickable, the current place bright.
    x += t.sidebar_gap * 2.0;
    let crumbs: Vec<(usize, &String)> =
        v.crumbs.iter().enumerate().flat_map(|(i, c)| c.iter().map(move |p| (i, p))).collect();
    let crumbs = if crumbs.is_empty() { vec![] } else { crumbs };
    let home = "Home".to_string();
    let parts: Vec<(Option<usize>, &String)> =
        if crumbs.is_empty() { vec![(None, &home)] } else { crumbs.iter().map(|(i, p)| (Some(*i), *p)).collect() };
    let last = parts.len() - 1;
    let right = full.right() - t.margin;
    for (k, (screen, label)) in parts.iter().enumerate() {
        if k > 0 {
            let g = ui.fonts(|f| f.layout_no_wrap("›".into(), t.font(t.heading_size), t.muted));
            ui.painter().galley(pos2(x, cy - g.size().y / 2.0), g.clone(), t.muted);
            x += g.size().x + t.sidebar_gap * 2.0;
        }
        let current = k == last;
        let ink = if current { t.text } else { t.sidebar_ink };
        let mut job = egui::text::LayoutJob::simple_singleline((*label).clone(), t.font(t.heading_size), ink);
        job.wrap = egui::text::TextWrapping::truncate_at_width((right - x).max(t.toolbar_button));
        let g = ui.fonts(|f| f.layout_job(job));
        let rect = Rect::from_min_size(pos2(x, cy - g.size().y / 2.0), g.size());
        let clickable = !current && screen.is_some();
        let r = ui.interact(
            rect.expand(t.sidebar_gap),
            ui.id().with(("nav-crumb", k)),
            if clickable { Sense::click() } else { Sense::hover() },
        );
        let ink = if clickable && r.hovered() { t.text } else { ink };
        ui.painter().galley(rect.min, g.clone(), ink);
        if clickable && r.hovered() {
            ui.painter().line_segment(
                [pos2(rect.left(), rect.bottom() + 1.0), pos2(rect.right(), rect.bottom() + 1.0)],
                Stroke::new(1.0, t.text),
            );
        }
        if clickable && r.clicked() {
            out = Some(Action::Crumb(screen.unwrap_or(0)));
        }
        x = rect.right() + t.sidebar_gap * 2.0;
        if x >= right {
            break;
        }
    }
    out
}

/// Painter-drawn sidebar icons (no icon font, no artwork), sized by `t.sidebar_icon`.
fn glyph(ui: &Ui, t: &Theme, g: Glyph, c: egui::Pos2, ink: Color32) {
    let p = ui.painter();
    let h = t.sidebar_icon / 2.0;
    let st = Stroke::new(t.sidebar_icon_stroke, ink);
    let r = |x0: f32, y0: f32, x1: f32, y1: f32| {
        Rect::from_min_max(pos2(c.x + x0 * h, c.y + y0 * h), pos2(c.x + x1 * h, c.y + y1 * h))
    };
    let line = |a: (f32, f32), b: (f32, f32)| {
        p.line_segment([pos2(c.x + a.0 * h, c.y + a.1 * h), pos2(c.x + b.0 * h, c.y + b.1 * h)], st)
    };
    let poly = |pts: &[(f32, f32)], closed: bool| {
        let v: Vec<egui::Pos2> = pts.iter().map(|(x, y)| pos2(c.x + x * h, c.y + y * h)).collect();
        p.add(if closed { Shape::closed_line(v, st) } else { Shape::line(v, st) });
    };
    let outline = |rect: Rect, rad: f32| p.rect_stroke(rect, rad, st, egui::StrokeKind::Middle);
    match g {
        Glyph::Sidebar => {
            outline(r(-1.0, -0.8, 1.0, 0.8), h * 0.25);
            line((-0.35, -0.8), (-0.35, 0.8));
        }
        Glyph::Home => poly(
            &[(-0.9, -0.05), (0.0, -0.9), (0.9, -0.05), (0.7, -0.05), (0.7, 0.85), (-0.7, 0.85), (-0.7, -0.05)],
            true,
        ),
        Glyph::Search => {
            p.circle_stroke(pos2(c.x - 0.15 * h, c.y - 0.15 * h), 0.6 * h, st);
            line((0.3, 0.3), (0.9, 0.9));
        }
        Glyph::Film => {
            outline(r(-0.9, -0.75, 0.9, 0.75), h * 0.2);
            line((-0.5, -0.75), (-0.5, 0.75));
            line((0.5, -0.75), (0.5, 0.75));
        }
        Glyph::Tv => {
            outline(r(-0.95, -0.7, 0.95, 0.5), h * 0.2);
            line((-0.45, 0.85), (0.45, 0.85));
        }
        Glyph::Music => {
            p.circle_filled(pos2(c.x - 0.35 * h, c.y + 0.55 * h), 0.3 * h, ink);
            line((-0.08, 0.55), (-0.08, -0.85));
            line((-0.08, -0.85), (0.65, -0.55));
        }
        Glyph::Folder => {
            poly(&[(-0.95, -0.7), (-0.25, -0.7), (-0.05, -0.45), (0.95, -0.45), (0.95, 0.75), (-0.95, 0.75)], true)
        }
        Glyph::Stack => {
            outline(r(-0.9, -0.35, 0.7, 0.85), h * 0.15);
            line((-0.65, -0.6), (0.85, -0.6));
            line((0.85, -0.6), (0.85, 0.55));
            line((-0.4, -0.85), (1.0, -0.85));
        }
        Glyph::List => {
            for y in [-0.6, 0.0, 0.6] {
                p.circle_filled(pos2(c.x - 0.8 * h, c.y + y * h), t.sidebar_icon_stroke, ink);
                line((-0.45, y), (0.9, y));
            }
        }
        Glyph::Heart => super::parts::heart(ui, t, c, t.sidebar_icon, false),
        Glyph::Server => {
            outline(r(-0.9, -0.85, 0.9, -0.1), h * 0.15);
            outline(r(-0.9, 0.1, 0.9, 0.85), h * 0.15);
            p.circle_filled(pos2(c.x + 0.55 * h, c.y - 0.47 * h), t.sidebar_icon_stroke, ink);
            p.circle_filled(pos2(c.x + 0.55 * h, c.y + 0.47 * h), t.sidebar_icon_stroke, ink);
        }
        Glyph::Back => poly(&[(0.35, -0.8), (-0.45, 0.0), (0.35, 0.8)], false),
        Glyph::Forward => poly(&[(-0.35, -0.8), (0.45, 0.0), (-0.35, 0.8)], false),
    }
}
