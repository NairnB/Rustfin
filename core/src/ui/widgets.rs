//! Reusable widget kit (W0): the building blocks shared by sort menus, subtitle settings, user settings
//! and every admin screen. Every value comes from `Theme`; each widget is keyboard/remote and mouse.
//!
//! Key handling for the dropdown, chip row, tabs, number field and sectioned list is a pure function
//! (`state + Key -> new state + event`), unit-tested below.
use super::parts;
use crate::platform::Key;
use crate::theme::Theme;
use egui::{pos2, vec2, Align2, Rect, Response, Sense, Ui};

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
    let mut out = DropdownOut {
        button_x: 0.0,
        item_xs: vec![],
        button_hovered: false,
        item_hovered: None,
        item_clicked: None,
        clicked_outside: false,
    };
    let current = options.get(state.highlighted).cloned().unwrap_or_default();
    let shown = if label.is_empty() { current } else { format!("{label}: {current}") };
    let button_focused = focused && !state.open;
    let r = parts::button_width(ui, t, &shown, button_focused, false, t.dropdown_w);
    out.button_x = r.rect.center().x;
    out.button_hovered = r.hovered();
    if r.clicked() {
        state.open = !state.open;
    }
    let mut menu_rects = vec![r.rect];
    if state.open {
        egui::ScrollArea::vertical()
            .id_salt("w0-dropdown-menu")
            .max_height(t.menu_max_h)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for (i, opt) in options.iter().enumerate() {
                    let item_focused = focused && state.highlighted == i;
                    let rr = menu_item(ui, t, opt, item_focused, t.dropdown_w);
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

/// A single menu row: left-aligned text, hover highlight, focus ring.
fn menu_item(ui: &mut Ui, t: &Theme, label: &str, focused: bool, width: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(width, t.menu_item_h), Sense::click());
    let fill = if resp.hovered() { t.surface_hi.gamma_multiply(1.3) } else { t.surface_hi };
    ui.painter().rect_filled(rect, t.radius, fill);
    let galley = ui.fonts(|f| f.layout_no_wrap(label.to_string(), t.font(t.body_size), t.text));
    let y = rect.top() + (t.menu_item_h - galley.size().y) / 2.0;
    ui.painter().galley(pos2(rect.left() + t.menu_pad, y), galley, t.text);
    if focused {
        parts::focus_ring(ui, t, rect);
    }
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
    let label_galley = ui.fonts(|f| f.layout_no_wrap(label.to_string(), t.font(t.body_size), t.text));
    let track = vec2(t.toggle_w, t.toggle_h);
    let width = label_galley.size().x + t.gap / 2.0 + track.x;
    let (rect, resp) = ui.allocate_exact_size(vec2(width, t.toggle_h), Sense::click());
    let label_y = rect.top() + (t.toggle_h - label_galley.size().y) / 2.0;
    ui.painter().galley(pos2(rect.left(), label_y), label_galley, t.text);
    let track_rect = Rect::from_min_size(pos2(rect.right() - track.x, rect.top()), track);
    let track_fill = if on { t.accent } else { t.surface_hi };
    ui.painter().rect_filled(track_rect, track.y / 2.0, track_fill);
    let d = t.toggle_h - 2.0 * t.toggle_inset;
    let thumb_color = if on { t.accent_text } else { t.muted };
    let thumb_x =
        if on { track_rect.right() - t.toggle_inset - d / 2.0 } else { track_rect.left() + t.toggle_inset + d / 2.0 };
    ui.painter().circle_filled(pos2(thumb_x, track_rect.center().y), d / 2.0, thumb_color);
    if focused {
        parts::focus_ring(ui, t, track_rect);
    }
    ToggleOut { x: track_rect.center().x, hovered: resp.hovered(), clicked: resp.clicked() }
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
            let galley = ui.fonts(|f| f.layout_no_wrap(label.clone(), t.font(t.body_size), t.text));
            let size = vec2(galley.size().x + 2.0 * t.chip_pad, t.chip_h);
            let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
            let on = selected.get(i).copied().unwrap_or(false);
            let (fill, ink) = if on { (t.accent, t.accent_text) } else { (t.surface_hi, t.text) };
            ui.painter().rect_filled(rect, t.chip_h / 2.0, fill);
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, label, t.font(t.body_size), ink);
            if focus == Some(i) {
                parts::focus_ring(ui, t, rect);
            }
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

/// A row of tab labels, the selected one marked with an underline.
pub fn tabs(ui: &mut Ui, t: &Theme, labels: &[String], selected: usize, focus: Option<usize>) -> TabsOut {
    let mut out = TabsOut { xs: vec![], hovered: None, clicked: None };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = t.gap / 2.0;
        for (i, label) in labels.iter().enumerate() {
            let galley = ui.fonts(|f| f.layout_no_wrap(label.clone(), t.font(t.body_size), t.text));
            let size = vec2(galley.size().x + 2.0 * t.tab_pad, t.tab_h);
            let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
            let ink = if i == selected { t.text } else { t.muted };
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, label, t.font(t.body_size), ink);
            if i == selected {
                let underline =
                    Rect::from_min_max(pos2(rect.left(), rect.bottom() - t.tab_underline), rect.right_bottom());
                ui.painter().rect_filled(underline, 0.0, t.accent);
            }
            if focus == Some(i) {
                parts::focus_ring(ui, t, rect);
            }
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
    ui.painter().rect_filled(rect, t.radius, t.surface);
    let galley = ui.fonts(|f| f.layout_no_wrap(text, t.font(t.body_size), t.text));
    let y = rect.top() + (t.button_h - galley.size().y) / 2.0;
    ui.painter().galley(pos2(rect.left() + t.button_pad, y), galley, t.text);
    if focused {
        parts::focus_ring(ui, t, rect);
    }
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
        let mut job = egui::text::LayoutJob::default();
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
            ui.painter().rect_filled(panel, t.radius, t.surface);
            let inner = panel.shrink(t.dialog_pad);
            ui.painter().galley(pos2(inner.left(), inner.top()), title_g, t.text);
            let body_top = inner.top() + title_h + t.gap / 2.0;
            ui.painter().galley(pos2(inner.left(), body_top), body_g, t.muted);
            let footer_top = inner.bottom() - t.button_h;
            let mut right = inner.right();
            let mut rects = vec![Rect::NOTHING; buttons.len()];
            for (i, label) in buttons.iter().enumerate() {
                let g = ui.fonts(|f| f.layout_no_wrap(label.clone(), t.font(t.body_size), t.text));
                let w = g.size().x + 2.0 * t.button_pad;
                rects[i] =
                    Rect::from_center_size(pos2(right - w / 2.0, footer_top + t.button_h / 2.0), vec2(w, t.button_h));
                right -= w + t.gap / 2.0;
            }
            for (i, rect) in rects.iter().enumerate() {
                let resp = ui.interact(*rect, ui.id().with(("w0-dialog-btn", i)), Sense::click());
                let primary = i == buttons.len() - 1;
                let (fill, ink) = if primary { (t.accent, t.accent_text) } else { (t.surface_hi, t.text) };
                let fill = if resp.hovered() && !primary { t.surface_hi.gamma_multiply(1.3) } else { fill };
                ui.painter().rect_filled(*rect, t.radius, fill);
                ui.painter().text(rect.center(), Align2::CENTER_CENTER, &buttons[i], t.font(t.body_size), ink);
                if focused == Some(i) {
                    parts::focus_ring(ui, t, *rect);
                }
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
pub fn list(ui: &mut Ui, t: &Theme, sections: &[(&str, Vec<ListRow<'_>>)], focus: Option<usize>) -> ListOut {
    let mut out = ListOut { xs: vec![], hovered: None, clicked: None };
    let mut idx = 0;
    for (heading, rows) in sections {
        ui.add_space(t.gap / 3.0);
        parts::muted(ui, t, heading);
        for row in rows {
            let focused = focus == Some(idx);
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), t.list_row_h), Sense::click());
            if ui.is_rect_visible(rect) {
                let has_secondary = row.secondary.is_some_and(|s| !s.is_empty());
                let label_g = ui.fonts(|f| f.layout_no_wrap(row.label.to_string(), t.font(t.body_size), t.text));
                let y = if has_secondary {
                    rect.top() + t.gap / 4.0
                } else {
                    rect.top() + (t.list_row_h - label_g.size().y) / 2.0
                };
                ui.painter().galley(pos2(rect.left(), y), label_g, t.text);
                if let Some(sec) = row.secondary.filter(|s| !s.is_empty()) {
                    let sec_g = ui.fonts(|f| f.layout_no_wrap(sec.to_string(), t.font(t.small_size), t.muted));
                    ui.painter().galley(pos2(rect.left(), y + t.body_size + 4.0), sec_g, t.muted);
                }
                if let Some(val) = row.value.filter(|v| !v.is_empty()) {
                    let val_g = ui.fonts(|f| f.layout_no_wrap(val.to_string(), t.font(t.body_size), t.muted));
                    let vy = rect.top() + (t.list_row_h - val_g.size().y) / 2.0;
                    ui.painter().galley(pos2(rect.right() - val_g.size().x, vy), val_g, t.muted);
                }
                if focused {
                    parts::focus_ring(ui, t, rect);
                }
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
        let r = parts::button(ui, t, "Save", save_focus, true);
        out.footer_xs.push(r.rect.center().x);
        if r.hovered() {
            out.button_hovered = Some(0);
        }
        if r.clicked() {
            out.button_clicked = Some(0);
        }
        let r = parts::button(ui, t, "Cancel", cancel_focus, false);
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
