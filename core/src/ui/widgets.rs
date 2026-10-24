//! Reusable widget kit (W0): the building blocks shared by sort menus, subtitle settings, user settings
//! and every admin screen. Every value comes from `Theme`; each widget is keyboard/remote and mouse.
//! The look is Apple TV (docs/DESIGN-appletv.md): pills, translucent menus, and focus as a white plate
//! that lifts with a soft shadow, eased over `focus_time`.
//!
//! Key handling for the dropdown, chip row, tabs, number field and sectioned list is a pure function
//! (`state + Key -> new state + event`), unit-tested below.
use super::parts;
use crate::platform::Key;
use crate::theme::{Theme, Weight};
use egui::{pos2, vec2, Align2, Color32, Id, Rect, Response, Sense, Stroke, Ui};

// ---------- 0. The look: focus motion, plates, pills, round icon buttons ----------

/// Eased focus amount 0..1 for `id`: 0 at rest, 1 focused, animated over `focus_time` (ease-out).
/// The first frame a control is drawn it starts at its target, so only focus moves animate. Focus marks
/// show only after keyboard/remote use (NAV): with the mouse, hover highlights do the job.
pub fn focus_amount(ui: &Ui, t: &Theme, id: Id, focused: bool) -> f32 {
    let on = focused && super::shell::keyboard_focus(ui.ctx());
    t.ease(ui.ctx().animate_bool_with_time(id.with("focus"), on, t.focus_time))
}

/// FIX-1 #8: `focus_amount` for text fields — shown whatever the input mode (after a mouse click too), because
/// the caret alone doesn't say where typing will go.
pub fn field_focus_amount(ui: &Ui, t: &Theme, id: Id, focused: bool) -> f32 {
    t.ease(ui.ctx().animate_bool_with_time(id.with("focus"), focused, t.focus_time))
}

/// `r` grown about its centre by the focus lift: `scale` at `a` = 1.
pub fn lift(r: Rect, scale: f32, a: f32) -> Rect {
    Rect::from_center_size(r.center(), r.size() * (1.0 + (scale - 1.0) * a))
}

/// The Apple TV focus plate: `rest` fill turning white, lifted by `scale`, with a soft shadow.
/// Returns the ink colour to draw on it (text → dark as it turns white).
pub fn plate(ui: &Ui, t: &Theme, rect: Rect, radius: f32, rest: Color32, a: f32, scale: f32) -> Color32 {
    let r = lift(rect, scale, a);
    if a > 0.01 {
        ui.painter().add(t.shadow(a).as_shape(r, radius));
    }
    ui.painter().rect_filled(r, radius, rest.lerp_to_gamma(t.focus_fill, a));
    // Ink flips at the halfway point: a blend would pass through grey-on-grey mid-animation.
    if a > 0.5 {
        t.focus_ink
    } else {
        t.text
    }
}

/// Small line icons drawn with the painter (no icon font, no Apple artwork).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Play,
    Plus,
    Tick,
    More,
    ChevronDown,
}

/// Draws `icon` centred on `c`, `size` = its box edge.
pub fn icon(ui: &Ui, t: &Theme, icon: Icon, c: egui::Pos2, size: f32, ink: Color32) {
    let p = ui.painter();
    let h = size / 2.0;
    let st = Stroke::new(t.icon_stroke, ink);
    match icon {
        Icon::Play => {
            let pts = vec![pos2(c.x - h * 0.7, c.y - h), pos2(c.x + h, c.y), pos2(c.x - h * 0.7, c.y + h)];
            p.add(egui::Shape::convex_polygon(pts, ink, Stroke::NONE));
        }
        Icon::Plus => {
            p.line_segment([pos2(c.x - h, c.y), pos2(c.x + h, c.y)], st);
            p.line_segment([pos2(c.x, c.y - h), pos2(c.x, c.y + h)], st);
        }
        Icon::Tick => {
            let pts = vec![pos2(c.x - h, c.y), pos2(c.x - h * 0.3, c.y + h * 0.7), pos2(c.x + h, c.y - h * 0.7)];
            p.add(egui::Shape::line(pts, st));
        }
        Icon::More => {
            for dx in [-h * 0.8, 0.0, h * 0.8] {
                p.circle_filled(pos2(c.x + dx, c.y), t.icon_stroke * 1.2, ink);
            }
        }
        Icon::ChevronDown => {
            let pts = vec![pos2(c.x - h, c.y - h * 0.5), pos2(c.x, c.y + h * 0.5), pos2(c.x + h, c.y - h * 0.5)];
            p.add(egui::Shape::line(pts, st));
        }
    }
}

fn pill_impl(
    ui: &mut Ui,
    t: &Theme,
    label: &str,
    focused: bool,
    primary: bool,
    size: Option<egui::Vec2>,
    lead: Option<Icon>,
) -> Response {
    let font = t.font_w(t.body_size, Weight::SemiBold);
    let g = ui.fonts(|f| f.layout_no_wrap(label.to_string(), font.clone(), Color32::PLACEHOLDER));
    let icon_w = if lead.is_some() { t.body_size + t.gap / 4.0 } else { 0.0 };
    let size = size.unwrap_or(vec2(g.size().x + icon_w + 2.0 * t.button_pad, t.button_h));
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let a = focus_amount(ui, t, resp.id, focused);
    // The primary pill rests slightly see-through so the focused one (solid, lifted) stands out.
    let rest = if primary {
        t.focus_fill.gamma_multiply(0.85)
    } else if resp.hovered() {
        t.glass_hi
    } else {
        t.glass
    };
    let ink = plate(ui, t, rect, size.y / 2.0, rest, a, t.pill_focus_scale);
    let ink = if primary { t.focus_ink } else { ink };
    let content_w = g.size().x + icon_w;
    let x0 = rect.center().x - content_w / 2.0;
    if let Some(i) = lead {
        icon(ui, t, i, pos2(x0 + t.body_size / 2.0, rect.center().y), t.body_size * 0.8, ink);
    }
    ui.painter().galley(pos2(x0 + icon_w, rect.center().y - g.size().y / 2.0), g, ink);
    resp
}

/// A pill button: white "primary" (Play/OK) or glass secondary; focused = white plate, lifted.
pub fn pill_button(ui: &mut Ui, t: &Theme, label: &str, focused: bool, primary: bool) -> Response {
    pill_impl(ui, t, label, focused, primary, None, None)
}

/// A pill button of a fixed width (dialog rows, dropdowns).
pub fn pill_button_width(ui: &mut Ui, t: &Theme, label: &str, focused: bool, primary: bool, width: f32) -> Response {
    pill_impl(ui, t, label, focused, primary, Some(vec2(width, t.button_h)), None)
}

/// How the hero Play pill is drawn (FIX-3 H1/H4): `white` = the plate is white in this state; `rest_white` =
/// it rests white even unfocused (desktop's primary pill); `scale` = the lift the plate reaches.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PillStyle {
    pub white: bool,
    pub rest_white: bool,
    pub scale: f32,
}

/// On TV only the focused Play is white (unfocused: translucent plate like the round buttons); desktop keeps
/// the white primary pill.
pub fn play_pill_style(t: &Theme, tv: bool, focused: bool) -> PillStyle {
    if tv {
        PillStyle { white: focused, rest_white: false, scale: if focused { t.pill_focus_scale } else { 1.0 } }
    } else {
        PillStyle { white: true, rest_white: true, scale: t.pill_focus_scale }
    }
}

/// The big hero pill: white, a play triangle and the label ("Play", "Resume").
pub fn play_pill(ui: &mut Ui, t: &Theme, label: &str, focused: bool) -> Response {
    play_pill_for(ui, t, label, focused, false)
}

/// `play_pill` for a target: on `tv` it is a glass plate that turns white only when focused.
pub fn play_pill_for(ui: &mut Ui, t: &Theme, label: &str, focused: bool, tv: bool) -> Response {
    let font = t.font_w(t.body_size, Weight::SemiBold);
    let w = ui.fonts(|f| f.layout_no_wrap(label.to_string(), font, t.text)).size().x;
    let size = vec2(w + t.body_size + t.gap / 4.0 + 2.0 * t.pill_pad, t.pill_h);
    let style = play_pill_style(t, tv, focused);
    pill_impl(ui, t, label, focused, style.rest_white, Some(size), Some(Icon::Play))
}

/// A round glass icon button (+ add, tick watched, more); focused = white plate, lifted.
pub fn round_button(ui: &mut Ui, t: &Theme, i: Icon, focused: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(t.round_button, t.round_button), Sense::click());
    let a = focus_amount(ui, t, resp.id, focused);
    let rest = if resp.hovered() { t.glass_hi } else { t.glass };
    let ink = plate(ui, t, rect, t.round_button / 2.0, rest, a, t.pill_focus_scale);
    icon(ui, t, i, rect.center(), t.round_button * 0.36, ink);
    resp
}

// ---------- 0b. Grouped cards (tvOS settings style) ----------

/// A section heading in `muted` above a rounded `surface` card holding rows (`kv_row`, `text_row`, or any
/// widgets). The card is as wide as the page up to `group_max_w`.
pub fn group<R>(ui: &mut Ui, t: &Theme, heading: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.add_space(t.gap / 2.0);
    if !heading.is_empty() {
        let g = ui.fonts(|f| f.layout_no_wrap(heading.to_string(), t.font_w(t.body_size, Weight::SemiBold), t.muted));
        let (r, _) = ui.allocate_exact_size(vec2(g.size().x, g.size().y + t.gap / 4.0), Sense::hover());
        ui.painter().galley(pos2(r.left() + t.section_pad, r.top()), g, t.muted);
    }
    let w = ui.available_width().min(t.group_max_w);
    egui::Frame::new()
        .fill(t.surface)
        .corner_radius(t.section_radius)
        .inner_margin(egui::Margin::symmetric(t.section_pad as i8, (t.gap / 4.0) as i8))
        .show(ui, |ui| {
            ui.set_width(w - 2.0 * t.section_pad);
            ui.spacing_mut().item_spacing.y = 0.0;
            add(ui)
        })
        .inner
}

/// One row inside a `group`: label on the left, value on the right in `muted`; a hairline under it unless `last`.
pub fn kv_row(ui: &mut Ui, t: &Theme, label: &str, value: &str, last: bool) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), t.list_row_h), Sense::hover());
    ui.painter().text(pos2(rect.left(), rect.center().y), Align2::LEFT_CENTER, label, t.font(t.body_size), t.text);
    ui.painter().text(pos2(rect.right(), rect.center().y), Align2::RIGHT_CENTER, value, t.font(t.body_size), t.muted);
    if !last {
        hairline(ui, t, rect);
    }
}

/// A plain text row inside a `group` (`muted` when `dim`); a hairline under it unless `last`.
pub fn text_row(ui: &mut Ui, t: &Theme, text: &str, dim: bool, last: bool) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), t.list_row_h), Sense::hover());
    let ink = if dim { t.muted } else { t.text };
    ui.painter().text(pos2(rect.left(), rect.center().y), Align2::LEFT_CENTER, text, t.font(t.body_size), ink);
    if !last {
        hairline(ui, t, rect);
    }
}

fn hairline(ui: &Ui, t: &Theme, rect: Rect) {
    let y = rect.bottom() - t.section_border_w / 2.0;
    ui.painter()
        .line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], Stroke::new(t.section_border_w, t.section_border));
}

// ---------- 1. Dropdown / menu ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DropdownState {
    pub open: bool,
    pub highlighted: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropdownEvent {
    Open,
    Close,
    Pick(usize),
}

/// Enter opens / picks; Up/Down move the highlight (no wrap, clamped); Back closes. `len` = option count.
pub fn dropdown_key(state: DropdownState, key: Key, len: usize) -> (DropdownState, Option<DropdownEvent>) {
    let n = len.max(1);
    match key {
        Key::Enter if !state.open => {
            (DropdownState { open: true, highlighted: state.highlighted.min(n - 1) }, Some(DropdownEvent::Open))
        }
        Key::Enter => (DropdownState { open: false, ..state }, Some(DropdownEvent::Pick(state.highlighted))),
        Key::Back if state.open => (DropdownState { open: false, ..state }, Some(DropdownEvent::Close)),
        Key::Up if state.open => (DropdownState { highlighted: state.highlighted.saturating_sub(1), ..state }, None),
        Key::Down if state.open => (DropdownState { highlighted: (state.highlighted + 1).min(n - 1), ..state }, None),
        _ => (state, None),
    }
}

pub struct DropdownOut {
    pub button_x: f32,
    pub item_xs: Vec<f32>,
    pub button_hovered: bool,
    pub item_hovered: Option<usize>,
    pub item_clicked: Option<usize>,
    pub clicked_outside: bool,
}

/// Button showing the current choice; when open, a menu below it. The button click toggles, an item click
/// picks (and closes), a click elsewhere closes. The caller passes `focused` = the button/current item.
pub fn dropdown(
    ui: &mut Ui,
    t: &Theme,
    state: &mut DropdownState,
    label: &str,
    options: &[String],
    focused: bool,
) -> DropdownOut {
    dropdown_impl(ui, t, state, label, options, focused, false)
}

/// `dropdown` as a full-width settings row (tvOS style): label on the left, the choice and a chevron on the
/// right; the menu opens under the row, right-aligned. For grouped cards (`group`).
pub fn dropdown_row(
    ui: &mut Ui,
    t: &Theme,
    state: &mut DropdownState,
    label: &str,
    options: &[String],
    focused: bool,
) -> DropdownOut {
    dropdown_impl(ui, t, state, label, options, focused, true)
}

fn dropdown_impl(
    ui: &mut Ui,
    t: &Theme,
    state: &mut DropdownState,
    label: &str,
    options: &[String],
    focused: bool,
    row: bool,
) -> DropdownOut {
    let mut out = DropdownOut {
        button_x: 0.0,
        item_xs: vec![],
        button_hovered: false,
        item_hovered: None,
        item_clicked: None,
        clicked_outside: false,
    };
    let current = options.get(state.highlighted).cloned().unwrap_or_default();
    let button_focused = focused && !state.open;
    let r = if row {
        setting_row(ui, t, label, &current, button_focused, RowEnd::Chevron)
    } else {
        let shown = if label.is_empty() { current } else { format!("{label}: {current}") };
        dropdown_button(ui, t, &shown, button_focused)
    };
    out.button_x = r.rect.center().x;
    out.button_hovered = r.hovered();
    if r.clicked() {
        state.open = !state.open;
    }
    let mut menu_rects = vec![r.rect];
    if state.open {
        // UX R10: the menu is a popover anchored under its button (right-aligned for rows), drawn over the page —
        // the page does not move when it opens.
        let inset = t.bar_inset;
        let (anchor, pivot) = if row {
            (r.rect.right_bottom() + vec2(0.0, t.gap / 4.0), egui::Align2::RIGHT_TOP)
        } else {
            (r.rect.left_bottom() + vec2(0.0, t.gap / 4.0), egui::Align2::LEFT_TOP)
        };
        egui::Area::new(r.id.with("w0-dropdown-popover"))
            .order(egui::Order::Foreground)
            .fixed_pos(anchor)
            .pivot(pivot)
            .constrain(true)
            .show(ui.ctx(), |ui| {
                egui::Frame::new()
                    .fill(t.menu_fill)
                    .corner_radius(t.menu_radius)
                    .stroke(Stroke::new(1.0, t.bar_stroke))
                    .shadow(t.shadow(1.0))
                    .inner_margin(egui::Margin::same(inset as i8))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        egui::ScrollArea::vertical()
                            .id_salt("w0-dropdown-menu")
                            .max_height(t.menu_max_h)
                            .auto_shrink([true, true])
                            .show(ui, |ui| {
                                for (i, opt) in options.iter().enumerate() {
                                    let item_focused = focused && state.highlighted == i;
                                    let rr = menu_item(ui, t, opt, item_focused, t.dropdown_w - 2.0 * inset);
                                    out.item_xs.push(rr.rect.center().x);
                                    menu_rects.push(rr.rect);
                                    if rr.hovered() {
                                        out.item_hovered = Some(i);
                                        state.highlighted = i;
                                    }
                                    if rr.clicked() {
                                        out.item_clicked = Some(i);
                                    }
                                }
                            });
                    });
            });
        if out.item_clicked.is_none() && ui.input(|i| i.pointer.any_click()) {
            if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                if !menu_rects.iter().any(|r| r.contains(pos)) {
                    state.open = false;
                    out.clicked_outside = true;
                }
            }
        }
    }
    out
}

/// The closed dropdown: a glass pill with the choice and a chevron.
fn dropdown_button(ui: &mut Ui, t: &Theme, label: &str, focused: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(t.dropdown_w, t.button_h), Sense::click());
    let a = focus_amount(ui, t, resp.id, focused);
    let rest = if resp.hovered() { t.glass_hi } else { t.glass };
    let ink = plate(ui, t, rect, t.button_h / 2.0, rest, a, t.pill_focus_scale);
    let g = ui.fonts(|f| f.layout_no_wrap(label.to_string(), t.font_w(t.body_size, Weight::Medium), ink));
    let y = rect.center().y - g.size().y / 2.0;
    ui.painter().galley(pos2(rect.left() + t.button_pad, y), g, ink);
    let c = pos2(rect.right() - t.button_pad, rect.center().y);
    icon(ui, t, Icon::ChevronDown, c, t.small_size * 0.8, ink);
    resp
}

/// A single menu row on the translucent menu card: the focused row is a white plate.
fn menu_item(ui: &mut Ui, t: &Theme, label: &str, focused: bool, width: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(width, t.menu_item_h), Sense::click());
    let a = focus_amount(ui, t, resp.id, focused);
    let rest = if resp.hovered() { t.glass } else { Color32::TRANSPARENT };
    let ink = plate(ui, t, rect, t.menu_radius - t.bar_inset, rest, a, 1.0);
    let galley = ui.fonts(|f| f.layout_no_wrap(label.to_string(), t.font(t.body_size), ink));
    let y = rect.top() + (t.menu_item_h - galley.size().y) / 2.0;
    ui.painter().galley(pos2(rect.left() + t.menu_pad, y), galley, ink);
    resp
}

// ---------- 2. Toggle ----------

pub struct ToggleOut {
    pub x: f32,
    pub hovered: bool,
    pub clicked: bool,
}

/// An on/off switch with a label. Enter/click flips (the caller flips on `clicked`).
pub fn toggle(ui: &mut Ui, t: &Theme, label: &str, on: bool, focused: bool) -> ToggleOut {
    let label_galley = ui.fonts(|f| f.layout_no_wrap(label.to_string(), t.font(t.body_size), Color32::PLACEHOLDER));
    let track = vec2(t.toggle_w, t.toggle_h);
    let pad = t.menu_pad;
    let width = pad + label_galley.size().x + t.gap / 2.0 + track.x + pad;
    let (rect, resp) = ui.allocate_exact_size(vec2(width, t.button_h), Sense::click());
    // The whole row is the focus target (label + switch), like a tvOS settings row.
    let a = focus_amount(ui, t, resp.id, focused);
    let ink = plate(ui, t, rect, t.button_h / 2.0, Color32::TRANSPARENT, a, t.pill_focus_scale);
    let label_y = rect.center().y - label_galley.size().y / 2.0;
    ui.painter().galley(pos2(rect.left() + pad, label_y), label_galley, ink);
    let track_rect = Rect::from_center_size(pos2(rect.right() - pad - track.x / 2.0, rect.center().y), track);
    let k = t.ease(ui.ctx().animate_bool_with_time(resp.id.with("on"), on, t.focus_time));
    let off_fill = t.glass_hi.lerp_to_gamma(t.surface_hi, a);
    ui.painter().rect_filled(track_rect, track.y / 2.0, off_fill.lerp_to_gamma(t.toggle_on, k));
    let d = t.toggle_h - 2.0 * t.toggle_inset;
    let x_off = track_rect.left() + t.toggle_inset + d / 2.0;
    let x_on = track_rect.right() - t.toggle_inset - d / 2.0;
    let knob = pos2(x_off + (x_on - x_off) * k, track_rect.center().y);
    ui.painter().add(t.shadow(0.3).as_shape(Rect::from_center_size(knob, vec2(d, d)), d / 2.0));
    ui.painter().circle_filled(knob, d / 2.0, t.focus_fill);
    ToggleOut { x: track_rect.center().x, hovered: resp.hovered(), clicked: resp.clicked() }
}

/// `toggle` as a full-width settings row (tvOS style): label on the left, the switch on the right; the whole row
/// is the focus target. For grouped cards (`group`).
pub fn toggle_row(ui: &mut Ui, t: &Theme, label: &str, on: bool, focused: bool) -> ToggleOut {
    let r = setting_row(ui, t, label, "", focused, RowEnd::Switch(on));
    ToggleOut { x: r.rect.center().x, hovered: r.hovered(), clicked: r.clicked() }
}

enum RowEnd {
    Chevron,
    Switch(bool),
}

/// One full-width settings row: label left; on the right a value + chevron, or a switch. Focused = white plate.
fn setting_row(ui: &mut Ui, t: &Theme, label: &str, value: &str, focused: bool, end: RowEnd) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), t.list_row_h), Sense::click());
    let a = focus_amount(ui, t, resp.id, focused);
    let plate_rect = rect.expand2(vec2(t.section_pad / 2.0, 0.0));
    let rest = if resp.hovered() { t.glass } else { Color32::TRANSPARENT };
    let ink = plate(ui, t, plate_rect, t.field_radius, rest, a, 1.0);
    let sub = if a > 0.5 { t.focus_ink.gamma_multiply(0.7) } else { t.muted };
    let font = t.font(t.body_size);
    ui.painter().text(pos2(rect.left(), rect.center().y), Align2::LEFT_CENTER, label, font.clone(), ink);
    match end {
        RowEnd::Chevron => {
            let c = pos2(rect.right() - t.small_size / 2.0, rect.center().y);
            icon(ui, t, Icon::ChevronDown, c, t.small_size * 0.8, sub);
            let x = rect.right() - t.small_size - t.gap / 3.0;
            ui.painter().text(pos2(x, rect.center().y), Align2::RIGHT_CENTER, value, font, sub);
        }
        RowEnd::Switch(on) => {
            let track = Rect::from_center_size(
                pos2(rect.right() - t.toggle_w / 2.0, rect.center().y),
                vec2(t.toggle_w, t.toggle_h),
            );
            let k = t.ease(ui.ctx().animate_bool_with_time(resp.id.with("on"), on, t.focus_time));
            let off_fill = if a > 0.5 { t.surface_hi } else { t.glass_hi };
            ui.painter().rect_filled(track, t.toggle_h / 2.0, off_fill.lerp_to_gamma(t.toggle_on, k));
            let d = t.toggle_h - 2.0 * t.toggle_inset;
            let x_off = track.left() + t.toggle_inset + d / 2.0;
            let x_on = track.right() - t.toggle_inset - d / 2.0;
            let knob = pos2(x_off + (x_on - x_off) * k, track.center().y);
            ui.painter().add(t.shadow(0.3).as_shape(Rect::from_center_size(knob, vec2(d, d)), d / 2.0));
            ui.painter().circle_filled(knob, d / 2.0, t.focus_fill);
        }
    }
    resp
}

// ---------- 3. Chip row ----------

/// Left/Right move the highlight (no wrap), Enter toggles. Returns (focus, chip to toggle).
pub fn chips_key(focus: usize, len: usize, key: Key) -> (usize, Option<usize>) {
    let n = len.max(1);
    let focus = focus.min(n - 1);
    match key {
        Key::Left => (focus.saturating_sub(1), None),
        Key::Right => ((focus + 1).min(n - 1), None),
        Key::Enter => (focus, Some(focus)),
        _ => (focus, None),
    }
}

pub struct ChipsOut {
    pub xs: Vec<f32>,
    pub hovered: Option<usize>,
    pub clicked: Option<usize>,
}

/// Several toggleable chips in a row. `selected` is parallel to `labels`; the caller flips on `clicked`.
pub fn chips(ui: &mut Ui, t: &Theme, labels: &[String], selected: &[bool], focus: Option<usize>) -> ChipsOut {
    let mut out = ChipsOut { xs: vec![], hovered: None, clicked: None };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = t.gap / 2.0;
        for (i, label) in labels.iter().enumerate() {
            let font = t.font_w(t.body_size, Weight::Medium);
            let galley = ui.fonts(|f| f.layout_no_wrap(label.clone(), font, Color32::PLACEHOLDER));
            let on = selected.get(i).copied().unwrap_or(false);
            // A selected chip carries a tick, so it still reads as selected while focused (white).
            let tick_w = if on { t.small_size + t.gap / 6.0 } else { 0.0 };
            let size = vec2(galley.size().x + tick_w + 2.0 * t.chip_pad, t.chip_h);
            let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
            let a = focus_amount(ui, t, resp.id, focus == Some(i));
            // FIX-2 #13: selection and focus never look alike — selected = an outline + tick on the resting glass,
            // focused = the white plate.
            let rest = if resp.hovered() { t.glass_hi } else { t.glass };
            let ink = plate(ui, t, rect, t.chip_h / 2.0, rest, a, t.pill_focus_scale);
            if on && a < 0.5 {
                let r = lift(rect, t.pill_focus_scale, a);
                ui.painter().rect_stroke(
                    r.shrink(t.badge_stroke / 2.0),
                    t.chip_h / 2.0,
                    Stroke::new(t.badge_stroke, t.text),
                    egui::StrokeKind::Inside,
                );
            }
            let x0 = rect.center().x - (galley.size().x + tick_w) / 2.0;
            if on {
                icon(ui, t, Icon::Tick, pos2(x0 + t.small_size / 2.0, rect.center().y), t.small_size * 0.8, ink);
            }
            ui.painter().galley(pos2(x0 + tick_w, rect.center().y - galley.size().y / 2.0), galley, ink);
            out.xs.push(rect.center().x);
            if resp.hovered() {
                out.hovered = Some(i);
            }
            if resp.clicked() {
                out.clicked = Some(i);
            }
        }
    });
    out
}

// ---------- 4. Tabs ----------

/// Left/Right switch (no wrap, clamped). Returns the new selected index.
pub fn tabs_key(selected: usize, len: usize, key: Key) -> usize {
    let n = len.max(1);
    let selected = selected.min(n - 1);
    match key {
        Key::Left => selected.saturating_sub(1),
        Key::Right => (selected + 1).min(n - 1),
        _ => selected,
    }
}

pub struct TabsOut {
    pub xs: Vec<f32>,
    pub hovered: Option<usize>,
    pub clicked: Option<usize>,
}

/// A pill segmented control on a translucent track: the selected segment is brighter glass, the focused
/// one a white plate (the same control as the top tab bar).
pub fn tabs(ui: &mut Ui, t: &Theme, labels: &[String], selected: usize, focus: Option<usize>) -> TabsOut {
    let mut out = TabsOut { xs: vec![], hovered: None, clicked: None };
    let font = t.font_w(t.body_size, Weight::Medium);
    let galleys: Vec<_> =
        labels.iter().map(|l| ui.fonts(|f| f.layout_no_wrap(l.clone(), font.clone(), Color32::PLACEHOLDER))).collect();
    let seg_h = t.tab_h - 2.0 * t.bar_inset;
    let widths: Vec<f32> = galleys.iter().map(|g| g.size().x + 2.0 * t.tab_pad).collect();
    let track_w = widths.iter().sum::<f32>() + 2.0 * t.bar_inset;
    let (track, _) = ui.allocate_exact_size(vec2(track_w, t.tab_h), Sense::hover());
    ui.painter().rect_filled(track, t.tab_h / 2.0, t.bar_fill);
    ui.painter().rect_stroke(track, t.tab_h / 2.0, Stroke::new(1.0, t.bar_stroke), egui::StrokeKind::Inside);
    let mut x = track.left() + t.bar_inset;
    for (i, galley) in galleys.into_iter().enumerate() {
        let rect = Rect::from_min_size(pos2(x, track.top() + t.bar_inset), vec2(widths[i], seg_h));
        x += widths[i];
        let resp = ui.interact(rect, ui.id().with(("w0-tab", i)), Sense::click());
        {
            let a = focus_amount(ui, t, resp.id, focus == Some(i));
            // FIX-2 #3: only focus gets a plate. The selected tab is white text with a short underline (tvOS), so
            // a page never shows two lit things; with the tabs focused it keeps a faint glass behind it.
            let rest =
                if (i == selected && focus.is_some()) || resp.hovered() { t.glass } else { Color32::TRANSPARENT };
            let ink = plate(ui, t, rect, seg_h / 2.0, rest, a, t.pill_focus_scale);
            let ink = if i == selected || a > 0.5 { ink } else { t.muted };
            let at = rect.center() - galley.size() / 2.0;
            if i == selected && a < 0.5 {
                selected_mark(ui, t, rect, galley.size().x, ink);
            }
            ui.painter().galley(at, galley, ink);
            out.xs.push(rect.center().x);
            if resp.hovered() {
                out.hovered = Some(i);
            }
            if resp.clicked() {
                out.clicked = Some(i);
            }
        }
    }
    out
}

/// FIX-2 #3: the selected-but-not-focused tab's mark — a short rounded underline under the label.
pub fn selected_mark(ui: &Ui, t: &Theme, rect: Rect, label_w: f32, ink: Color32) {
    let w = (label_w * 0.6).max(t.gap / 2.0);
    let y = rect.bottom() - t.bar_inset;
    let r = Rect::from_center_size(pos2(rect.center().x, y), vec2(w, t.badge_stroke * 2.0));
    ui.painter().rect_filled(r, t.badge_stroke, ink);
}

// ---------- 5. Text field (wraps parts) and number field ----------

/// Wraps `parts::text_field` so the widget kit is one place to reach for fields.
pub fn text_field(ui: &mut Ui, t: &Theme, text: &mut String, hint: &str, focused: bool, password: bool) -> Response {
    parts::text_field(ui, t, text, hint, focused, password)
}

/// Digits append; Up/Down step; the result is clamped to `[min, max]`.
pub fn number_key(value: i64, key: Key, min: i64, max: i64, step: i64) -> i64 {
    let step = step.max(1);
    let next = match key {
        Key::Text(c) if c.is_ascii_digit() => {
            value.saturating_mul(10).saturating_add(c.to_digit(10).unwrap_or(0) as i64)
        }
        Key::Up => value.saturating_add(step),
        Key::Down => value.saturating_sub(step),
        _ => value,
    };
    next.clamp(min, max)
}

/// A digit-only field drawn like `parts::text_field` but driven by an integer value (no real TextEdit, so
/// typed digits come through the pure `number_key` function in the shell that feeds `Key::Text`).
pub fn number_field(ui: &mut Ui, t: &Theme, value: i64, focused: bool) -> Response {
    let text = value.to_string();
    let (rect, resp) = ui.allocate_exact_size(vec2(t.field_w, t.button_h), Sense::click());
    let a = field_focus_amount(ui, t, resp.id, focused);
    let ink = plate(ui, t, rect, t.field_radius, t.surface_hi, a, 1.0);
    let galley = ui.fonts(|f| f.layout_no_wrap(text, t.font(t.body_size), ink));
    let y = rect.top() + (t.button_h - galley.size().y) / 2.0;
    ui.painter().galley(pos2(rect.left() + t.button_pad, y), galley, ink);
    resp
}

// ---------- 6. Dialog / confirm ----------

pub struct DialogOut {
    pub button_xs: Vec<f32>,
    pub hovered: Option<usize>,
    pub clicked: Option<usize>,
}

/// A centred panel over a scrim: title, body, then a row of buttons (OK/Cancel or custom) right-aligned.
/// `focused` = the focused button index (the caller handles Back = cancel and the click result).
pub fn dialog(
    ui: &mut Ui,
    t: &Theme,
    title: &str,
    body: &str,
    buttons: &[String],
    focused: Option<usize>,
) -> DialogOut {
    let mut out = DialogOut { button_xs: vec![], hovered: None, clicked: None };
    let ctx = ui.ctx().clone();
    let screen = ctx.screen_rect();
    // The scrim is a full-window middle layer: above the page, below the dialog (Foreground).
    ctx.layer_painter(egui::LayerId::new(egui::Order::Middle, egui::Id::new("w0-scrim")))
        .rect_filled(screen, 0.0, t.scrim);
    let title_g = ctx.fonts(|f| f.layout_no_wrap(title.to_string(), t.font(t.heading_size), t.text));
    let body_g = ctx.fonts(|f| {
        let mut job = egui::text::LayoutJob { halign: egui::Align::Center, ..Default::default() };
        job.append(body, 0.0, egui::TextFormat { font_id: t.font(t.body_size), color: t.muted, ..Default::default() });
        job.wrap = egui::text::TextWrapping {
            max_width: t.dialog_w - 2.0 * t.dialog_pad,
            max_rows: 4,
            break_anywhere: false,
            overflow_character: None,
        };
        f.layout_job(job)
    });
    let title_h = title_g.size().y;
    let body_h = body_g.size().y;
    let panel_h = title_h + t.gap / 2.0 + body_h + t.gap + t.button_h + 2.0 * t.dialog_pad;
    let panel_size = vec2(t.dialog_w, panel_h);

    egui::Area::new(egui::Id::new("w0-dialog"))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.center())
        .pivot(egui::Align2::CENTER_CENTER)
        .show(&ctx, |ui| {
            let (panel, _) = ui.allocate_exact_size(panel_size, Sense::hover());
            // Translucent material card, centred text, equal-width pills in index order (left to right).
            ui.painter().add(t.shadow(1.0).as_shape(panel, t.dialog_radius));
            ui.painter().rect_filled(panel, t.dialog_radius, t.menu_fill);
            ui.painter().rect_stroke(panel, t.dialog_radius, Stroke::new(1.0, t.bar_stroke), egui::StrokeKind::Inside);
            let inner = panel.shrink(t.dialog_pad);
            ui.painter().galley(pos2(inner.center().x - title_g.size().x / 2.0, inner.top()), title_g, t.text);
            let body_top = inner.top() + title_h + t.gap / 2.0;
            ui.painter().galley(pos2(inner.center().x, body_top), body_g, t.muted);
            let footer_top = inner.bottom() - t.button_h;
            let n = buttons.len().max(1) as f32;
            let gap = t.gap / 2.0;
            let w = ((inner.width() - gap * (n - 1.0)) / n).min(t.dropdown_w);
            let row_w = w * n + gap * (n - 1.0);
            let mut rects = vec![Rect::NOTHING; buttons.len()];
            for (i, r) in rects.iter_mut().enumerate() {
                let x = inner.center().x - row_w / 2.0 + i as f32 * (w + gap);
                *r = Rect::from_min_size(pos2(x, footer_top), vec2(w, t.button_h));
            }
            let font = t.font_w(t.body_size, Weight::SemiBold);
            for (i, rect) in rects.iter().enumerate() {
                let resp = ui.interact(*rect, ui.id().with(("w0-dialog-btn", i)), Sense::click());
                let a = focus_amount(ui, t, resp.id, focused == Some(i));
                let rest = if resp.hovered() { t.glass_hi } else { t.glass };
                let ink = plate(ui, t, *rect, t.button_h / 2.0, rest, a, t.pill_focus_scale);
                ui.painter().text(rect.center(), Align2::CENTER_CENTER, &buttons[i], font.clone(), ink);
                out.button_xs.push(rect.center().x);
                if resp.hovered() {
                    out.hovered = Some(i);
                }
                if resp.clicked() {
                    out.clicked = Some(i);
                }
            }
        });
    out
}

// ---------- 7. List with sections ----------

/// Up/Down move the focus (no wrap, clamped), Enter activates. Returns (focus, activated row).
pub fn list_key(focus: usize, len: usize, key: Key) -> (usize, Option<usize>) {
    let n = len.max(1);
    let focus = focus.min(n - 1);
    match key {
        Key::Up => (focus.saturating_sub(1), None),
        Key::Down => ((focus + 1).min(n - 1), None),
        Key::Enter => (focus, Some(focus)),
        _ => (focus, None),
    }
}

pub struct ListRow<'a> {
    pub label: &'a str,
    pub secondary: Option<&'a str>,
    pub value: Option<&'a str>,
}

pub struct ListOut {
    pub xs: Vec<f32>,
    pub hovered: Option<usize>,
    pub clicked: Option<usize>,
}

/// Headings + rows. `focus` indexes the flattened rows across all sections (headings are not focusable).
/// Each section is a grouped card (tvOS settings style): rows with hairlines, the focused row a white plate.
pub fn list(ui: &mut Ui, t: &Theme, sections: &[(&str, Vec<ListRow<'_>>)], focus: Option<usize>) -> ListOut {
    list_impl(ui, t, sections, focus, false)
}

/// `list`, and the focused row is scrolled into view (for lists longer than a screen).
pub fn list_scrolling(ui: &mut Ui, t: &Theme, sections: &[(&str, Vec<ListRow<'_>>)], focus: Option<usize>) -> ListOut {
    list_impl(ui, t, sections, focus, true)
}

fn list_impl(
    ui: &mut Ui,
    t: &Theme,
    sections: &[(&str, Vec<ListRow<'_>>)],
    focus: Option<usize>,
    scroll: bool,
) -> ListOut {
    let mut out = ListOut { xs: vec![], hovered: None, clicked: None };
    let mut idx = 0;
    for (heading, rows) in sections {
        if rows.is_empty() {
            continue;
        }
        group(ui, t, heading, |ui| {
            let n = rows.len();
            for (k, row) in rows.iter().enumerate() {
                let focused = focus == Some(idx);
                let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), t.list_row_h), Sense::click());
                let a = focus_amount(ui, t, resp.id, focused);
                if ui.is_rect_visible(rect) {
                    // The plate reaches the card's inner edges; text keeps the card's padding.
                    let plate_rect = rect.expand2(vec2(t.section_pad / 2.0, 0.0));
                    let rest = if resp.hovered() { t.glass } else { Color32::TRANSPARENT };
                    let ink = plate(ui, t, plate_rect, t.field_radius, rest, a, 1.0);
                    let sub = if a > 0.5 { t.focus_ink.gamma_multiply(0.7) } else { t.muted };
                    let has_secondary = row.secondary.is_some_and(|s| !s.is_empty());
                    let label_g = ui.fonts(|f| f.layout_no_wrap(row.label.to_string(), t.font(t.body_size), ink));
                    let y = if has_secondary {
                        rect.top() + t.gap / 4.0
                    } else {
                        rect.top() + (t.list_row_h - label_g.size().y) / 2.0
                    };
                    ui.painter().galley(pos2(rect.left(), y), label_g, ink);
                    if let Some(sec) = row.secondary.filter(|s| !s.is_empty()) {
                        let sec_g = ui.fonts(|f| f.layout_no_wrap(sec.to_string(), t.font(t.small_size), sub));
                        ui.painter().galley(pos2(rect.left(), y + t.body_size + 4.0), sec_g, sub);
                    }
                    if let Some(val) = row.value.filter(|v| !v.is_empty()) {
                        let val_g = ui.fonts(|f| f.layout_no_wrap(val.to_string(), t.font(t.body_size), sub));
                        let vy = rect.top() + (t.list_row_h - val_g.size().y) / 2.0;
                        ui.painter().galley(pos2(rect.right() - val_g.size().x, vy), val_g, sub);
                    }
                    if k + 1 < n && a < 0.01 {
                        hairline(ui, t, rect);
                    }
                }
                if scroll && focused {
                    ui.scroll_to_rect(rect.expand(t.gap), None);
                }
                out.xs.push(rect.center().x);
                if resp.hovered() {
                    out.hovered = Some(idx);
                }
                if resp.clicked() {
                    out.clicked = Some(idx);
                }
                idx += 1;
            }
        });
    }
    out
}

// ---------- 8. Form layout ----------

pub struct FormOut {
    pub field_xs: Vec<f32>,
    pub footer_xs: Vec<f32>,
    pub field_hovered: Option<usize>,
    pub field_clicked: Option<usize>,
    pub button_hovered: Option<usize>,
    pub button_clicked: Option<usize>,
}

/// A label column + field column, a section heading, and a Save/Cancel footer. Below `form_breakpoint`
/// the two columns narrow to one. `focus` = the focus index: fields first, then Save, then Cancel.
pub fn form(ui: &mut Ui, t: &Theme, heading: &str, rows: &mut [(String, String)], focus: Option<usize>) -> FormOut {
    let mut out = FormOut {
        field_xs: vec![],
        footer_xs: vec![],
        field_hovered: None,
        field_clicked: None,
        button_hovered: None,
        button_clicked: None,
    };
    parts::heading(ui, t, heading);
    ui.add_space(t.gap / 3.0);
    let n = rows.len();
    let wide = ui.available_width() >= t.form_breakpoint;
    for (i, (label, value)) in rows.iter_mut().enumerate() {
        let field_focused = focus == Some(i);
        if wide {
            ui.horizontal(|ui| {
                ui.add_space(t.gap);
                let g = ui.fonts(|f| f.layout_no_wrap(label.clone(), t.font(t.body_size), t.text));
                let (label_rect, _) = ui.allocate_exact_size(vec2(t.form_label_w, t.button_h), Sense::hover());
                ui.painter().galley(pos2(label_rect.left(), label_rect.center().y), g, t.text);
                let r = text_field(ui, t, value, label.as_str(), field_focused, false);
                out.field_xs.push(r.rect.center().x);
                if r.hovered() {
                    out.field_hovered = Some(i);
                }
                if r.clicked() {
                    out.field_clicked = Some(i);
                }
            });
        } else {
            parts::muted(ui, t, label.as_str());
            let r = text_field(ui, t, value, label.as_str(), field_focused, false);
            out.field_xs.push(r.rect.center().x);
            if r.hovered() {
                out.field_hovered = Some(i);
            }
            if r.clicked() {
                out.field_clicked = Some(i);
            }
        }
        ui.add_space(t.gap / 3.0);
    }
    ui.add_space(t.gap / 2.0);
    let save_focus = focus == Some(n);
    let cancel_focus = focus == Some(n + 1);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = t.gap / 2.0;
        let r = pill_button(ui, t, "Save", save_focus, true);
        out.footer_xs.push(r.rect.center().x);
        if r.hovered() {
            out.button_hovered = Some(0);
        }
        if r.clicked() {
            out.button_clicked = Some(0);
        }
        let r = pill_button(ui, t, "Cancel", cancel_focus, false);
        out.footer_xs.push(r.rect.center().x);
        if r.hovered() {
            out.button_hovered = Some(1);
        }
        if r.clicked() {
            out.button_clicked = Some(1);
        }
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropdown_enter_opens_then_picks() {
        let s = DropdownState::default();
        let (s, ev) = dropdown_key(s, Key::Enter, 3);
        assert!(s.open);
        assert_eq!(ev, Some(DropdownEvent::Open));
        let (s, ev) = dropdown_key(s, Key::Down, 3);
        assert!(s.open);
        assert_eq!(s.highlighted, 1);
        assert_eq!(ev, None);
        let (s, ev) = dropdown_key(s, Key::Enter, 3);
        assert!(!s.open);
        assert_eq!(ev, Some(DropdownEvent::Pick(1)));
    }

    #[test]
    fn dropdown_highlight_clamps_without_wrapping() {
        let s = DropdownState { open: true, highlighted: 0 };
        let (s, _) = dropdown_key(s, Key::Up, 4);
        assert_eq!(s.highlighted, 0);
        let s = DropdownState { open: true, highlighted: 3 };
        let (s, _) = dropdown_key(s, Key::Down, 4);
        assert_eq!(s.highlighted, 3);
    }

    #[test]
    fn dropdown_opens_on_the_current_choice() {
        // FIX-1 #15: Enter opens the menu with the current choice highlighted (no extra press to reach it).
        let s = DropdownState { open: false, highlighted: 2 };
        let (s, ev) = dropdown_key(s, Key::Enter, 4);
        assert_eq!((s.open, s.highlighted, ev), (true, 2, Some(DropdownEvent::Open)));
    }

    #[test]
    fn dropdown_back_closes() {
        let s = DropdownState { open: true, highlighted: 2 };
        let (s, ev) = dropdown_key(s, Key::Back, 4);
        assert!(!s.open);
        assert_eq!(ev, Some(DropdownEvent::Close));
        let (s, ev) = dropdown_key(s, Key::Back, 4);
        assert!(!s.open);
        assert_eq!(ev, None);
    }

    #[test]
    fn chips_move_without_wrapping_and_toggle_on_enter() {
        let (f, ev) = chips_key(0, 3, Key::Left);
        assert_eq!(f, 0);
        assert_eq!(ev, None);
        let (f, ev) = chips_key(0, 3, Key::Right);
        assert_eq!(f, 1);
        assert_eq!(ev, None);
        let (f, _) = chips_key(f, 3, Key::Right);
        assert_eq!(f, 2);
        let (f, ev) = chips_key(f, 3, Key::Right);
        assert_eq!(f, 2);
        assert_eq!(ev, None);
        let (f, ev) = chips_key(1, 3, Key::Enter);
        assert_eq!(f, 1);
        assert_eq!(ev, Some(1));
    }

    #[test]
    fn tabs_switch_without_wrapping() {
        assert_eq!(tabs_key(0, 4, Key::Left), 0);
        assert_eq!(tabs_key(0, 4, Key::Right), 1);
        assert_eq!(tabs_key(3, 4, Key::Right), 3);
        assert_eq!(tabs_key(2, 4, Key::Left), 1);
    }

    #[test]
    fn number_field_digits_step_and_clamp() {
        assert_eq!(number_key(0, Key::Text('5'), 0, 100, 5), 5);
        assert_eq!(number_key(5, Key::Text('3'), 0, 100, 5), 53);
        assert_eq!(number_key(53, Key::Up, 0, 100, 5), 58);
        assert_eq!(number_key(3, Key::Down, 0, 100, 5), 0);
        assert_eq!(number_key(98, Key::Up, 0, 100, 5), 100);
        assert_eq!(number_key(100, Key::Text('0'), 0, 100, 5), 100);
        assert_eq!(number_key(0, Key::Text('x'), 0, 100, 5), 0);
    }

    #[test]
    fn list_moves_without_wrapping_and_activates() {
        let (f, ev) = list_key(0, 5, Key::Up);
        assert_eq!(f, 0);
        assert_eq!(ev, None);
        let (f, _) = list_key(4, 5, Key::Down);
        assert_eq!(f, 4);
        let (f, ev) = list_key(2, 5, Key::Enter);
        assert_eq!(f, 2);
        assert_eq!(ev, Some(2));
    }
}
