//! UXP5: the TV's on-screen keyboard — a tvOS-style grid (a digit row, then 3 rows of keys, each with a side key —
//! shift / 123 / clear — at its left, so shift and 123 are two presses from the first letter; then space /
//! delete / done). The model (`KeyboardState::press_at` → `Effect`) is pure and unit-tested; `draw` paints it.
//! Only the TV build opens it (see `App::tv_keys`); typed keys reach the focused field as egui text events, so a
//! field needs no code of its own — it only has to call `register` (done once, in `parts::text_field`).
use super::widgets;
use crate::platform::Key;
use crate::theme::{Theme, Weight};
use egui::{pos2, vec2, Context, Id, Rect, Sense};

/// What one key does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Cap {
    Char(char),
    Space,
    Delete,
    Clear,
    Shift,
    /// letters ↔ numbers and symbols
    Layer,
    Done,
}

/// A key and its width in cells (a letter is 1; the grid is 10 cells wide).
#[derive(Clone, Copy, Debug)]
pub struct Cell {
    pub cap: Cap,
    pub w: f32,
}

/// What a key press asks the app to do.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Effect {
    None,
    Type(char),
    Delete,
    Clear,
    /// Close the keyboard, keeping the text.
    Close,
    /// Down from the last row or Up from the first (FIX-2 #1a): close the keyboard and give this key to the screen
    /// (focus moves on — to the results or the field).
    Leave(Key),
}

/// FIX-4 K1: both layers open with the digit row, so names like `view-pass-1` never need the 123 layer.
const LETTERS: [&str; 4] = ["1234567890", "abcdefghij", "klmnopqrst", "uvwxyz.-_@"];
const SYMBOLS: [&str; 4] = ["1234567890", ":/.-_@?&=+", "!#$%^*()~,", "[]{}<>|;'\""];

/// Width of the side-key column, in cells (the grid is `SIDE + 10` wide).
const SIDE: f32 = 1.5;
/// The side keys of the rows under the digit row; the digit row has none and spreads its keys over the full width.
const SIDE_KEYS: [Cap; 3] = [Cap::Shift, Cap::Layer, Cap::Clear];

pub fn layout(symbols: bool, shift: bool) -> Vec<Vec<Cell>> {
    let src = if symbols { &SYMBOLS } else { &LETTERS };
    let mut rows: Vec<Vec<Cell>> =
        src.iter()
            .enumerate()
            .map(|(i, r)| {
                let (side, w) = match i.checked_sub(1) {
                    Some(s) => (Some(SIDE_KEYS[s]), 1.0),
                    None => (None, (SIDE + 10.0) / 10.0),
                };
                side.map(|cap| Cell { cap, w: SIDE })
                    .into_iter()
                    .chain(r.chars().map(|c| Cell {
                        cap: Cap::Char(if shift && !symbols { c.to_ascii_uppercase() } else { c }),
                        w,
                    }))
                    .collect()
            })
            .collect();
    let bottom = [(Cap::Space, 4.5), (Cap::Delete, 3.5), (Cap::Done, 3.5)];
    rows.push(bottom.iter().map(|&(cap, w)| Cell { cap, w }).collect());
    rows
}

pub fn label(cap: Cap, symbols: bool, shift: bool) -> String {
    match cap {
        Cap::Char(c) => c.to_string(),
        Cap::Space => "space".into(),
        Cap::Delete => "delete".into(),
        Cap::Clear => "clear".into(),
        Cap::Shift => if shift { "SHIFT ON" } else { "shift" }.into(),
        Cap::Layer => if symbols { "abc" } else { "123" }.into(),
        Cap::Done => "done".into(),
    }
}

/// K3: the key is drawn filled (not just glass) — Shift while it is on.
pub fn key_filled(st: &KeyboardState, cap: Cap) -> bool {
    cap == Cap::Shift && st.shift
}

/// K3: how Shift-on is drawn: a tinted glass plate with an outline — never the white plate of the focused key
/// (only the focused thing is lit). Returns (fill, outline width).
pub fn on_look(t: &Theme) -> (egui::Color32, f32) {
    (t.glass_hi, 2.0)
}

/// FIX-4 K3: a key's look. Only the focused key is white (and lifted by its own `osk_focus_scale`); every other key
/// is dark glass with dimmed text. Returns (fill, ink, scale).
pub fn key_look(t: &Theme, focused: bool, on: bool) -> (egui::Color32, egui::Color32, f32) {
    if focused {
        (t.focus_fill, t.focus_ink, t.osk_focus_scale)
    } else {
        (if on { on_look(t).0 } else { t.glass }, dim(t.text, t.osk_dim), 1.0)
    }
}

/// `c` with its brightness scaled by `k`.
fn dim(c: egui::Color32, k: f32) -> egui::Color32 {
    let f = |v: u8| (v as f32 * k).round() as u8;
    egui::Color32::from_rgb(f(c.r()), f(c.g()), f(c.b()))
}

/// FIX-4 K2: the panel's rectangle: at most `osk_w` wide, centred, its bottom `osk_bottom` above the screen edge.
pub fn panel_rect(t: &Theme, screen: Rect, rows: usize) -> Rect {
    let w = (screen.width() - 2.0 * t.margin).min(t.osk_w);
    let h = 2.0 * (t.gap / 3.0) + t.osk_text_h + rows as f32 * (t.osk_key_h + t.osk_gap);
    Rect::from_min_size(pos2(screen.center().x - w / 2.0, screen.bottom() - t.osk_bottom - h), vec2(w, h))
}

/// FIX-4 (review 3): the height the page gives up while the keyboard is open (panel plus its bottom margin), so the
/// field and the first results or buttons stay in view above it.
pub fn reserved_height(t: &Theme, rows: usize) -> f32 {
    panel_rect(t, Rect::from_min_size(pos2(0.0, 0.0), vec2(t.osk_w + 2.0 * t.margin, 2000.0)), rows).height()
        + t.osk_bottom
}

/// Centre of column `col` of `row`, in cells.
fn centre(row: &[Cell], col: usize) -> f32 {
    row.iter().take(col).map(|c| c.w).sum::<f32>() + row.get(col).map_or(0.0, |c| c.w / 2.0)
}

/// The column of `row` whose centre is nearest `x`.
fn nearest(row: &[Cell], x: f32) -> usize {
    (0..row.len()).min_by(|&a, &b| (centre(row, a) - x).abs().total_cmp(&(centre(row, b) - x).abs())).unwrap_or(0)
}

#[derive(Clone, Debug)]
pub struct KeyboardState {
    pub row: usize,
    pub col: usize,
    pub shift: bool,
    pub symbols: bool,
    /// K1: a name field — Shift comes on again for the first letter and after each space.
    title_case: bool,
    /// Where (in cells) the focus was last moved sideways: Up/Down keep to it across rows of different shapes.
    x: f32,
}

impl Default for KeyboardState {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyboardState {
    pub fn new() -> Self {
        // Opens on the first letter, "a" (row 0 is the digits; the side key is column 0).
        KeyboardState { row: 1, col: 1, shift: false, symbols: false, title_case: false, x: SIDE + 0.5 }
    }

    /// A keyboard for `f`: a name field with nothing typed (or ending in a space) starts on capitals (K1); user
    /// name, password, address and search never do (K2).
    pub fn for_field(f: &TvField) -> Self {
        let mut k = Self::new();
        k.title_case = f.name;
        k.shift = f.name && f.text.chars().last().is_none_or(char::is_whitespace);
        k
    }

    /// FIX-5 R2: the keyboard for `f` again after it was closed on `saved`: the same key and layer, with Shift set by
    /// the K1/K2 rules for the text as it now is. A field with nothing typed starts on "a" as `for_field` does (R3).
    pub fn for_field_resume(f: &TvField, saved: &KeyboardState) -> Self {
        let mut k = Self::for_field(f);
        if f.text.is_empty() {
            return k;
        }
        k.symbols = saved.symbols;
        let rows = k.layout();
        k.row = saved.row.min(rows.len() - 1);
        k.col = saved.col.min(rows[k.row].len() - 1);
        k.x = saved.x;
        k
    }

    pub fn layout(&self) -> Vec<Vec<Cell>> {
        layout(self.symbols, self.shift)
    }

    pub fn focused_cap(&self) -> Cap {
        self.layout()[self.row][self.col].cap
    }

    /// K1: after the field's text changed by a delete, Shift follows the text: on for the first letter and after a space.
    pub fn retitle(&mut self, text: &str) {
        if self.title_case {
            self.shift = text.chars().last().is_none_or(char::is_whitespace);
        }
    }

    /// `press_at` without a clock.
    pub fn press(&mut self, key: Key) -> Effect {
        self.press_at(key, None)
    }

    /// One key from the remote. Every press moves exactly one key (FIX-2 #1b: the old time-based speed-up also
    /// caught quick separate taps and overshot; a held key's own auto-repeat is quick enough). `now` is kept for
    /// callers and unused.
    pub fn press_at(&mut self, key: Key, _now: Option<f64>) -> Effect {
        let rows = self.layout();
        let steps = 1;
        match key {
            Key::Left | Key::Right => {
                let len = rows[self.row].len();
                self.col =
                    if key == Key::Left { self.col.saturating_sub(steps) } else { (self.col + steps).min(len - 1) };
                self.x = centre(&rows[self.row], self.col);
                Effect::None
            }
            Key::Up => {
                if self.row > 0 {
                    self.row -= 1;
                    self.col = nearest(&rows[self.row], self.x);
                    Effect::None
                } else {
                    // FIX-2 #1a: Up from the top row leaves the keyboard (it was a trap: only Back or "done" left).
                    Effect::Leave(Key::Up)
                }
            }
            Key::Down => {
                if self.row + 1 < rows.len() {
                    self.row += 1;
                    self.col = nearest(&rows[self.row], self.x);
                    Effect::None
                } else {
                    Effect::Leave(Key::Down)
                }
            }
            Key::Enter => self.activate(),
            Key::Back => Effect::Close,
            Key::Delete | Key::Backspace => Effect::Delete,
            _ => Effect::None,
        }
    }

    /// Enter (or a click) on the focused key.
    pub fn activate(&mut self) -> Effect {
        match self.focused_cap() {
            Cap::Char(c) => {
                self.shift = false; // shift is for one letter
                Effect::Type(c)
            }
            Cap::Space => {
                self.shift = self.title_case;
                Effect::Type(' ')
            }
            Cap::Delete => Effect::Delete,
            Cap::Clear => {
                self.shift = self.title_case;
                Effect::Clear
            }
            Cap::Shift => {
                self.shift = !self.shift;
                Effect::None
            }
            Cap::Layer => {
                self.symbols = !self.symbols;
                Effect::None
            }
            Cap::Done => Effect::Close,
        }
    }
}

/// Types `e` into the focused field as egui events (`text` = the field's text before it): a letter, a delete (Shift
/// follows the text again) or a clear. The remote's keys and a mouse click on a key both come through here.
/// Returns false for the effects that are not about text (None, Close, Leave).
pub fn apply_text(ctx: &Context, kb: &mut KeyboardState, text: &str, e: Effect) -> bool {
    let backspace = |n: usize| {
        ctx.input_mut(|i| {
            for _ in 0..n {
                i.events.push(egui::Event::Key {
                    key: egui::Key::Backspace,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Default::default(),
                });
            }
        })
    };
    match e {
        Effect::Type(c) => ctx.input_mut(|i| i.events.push(egui::Event::Text(c.to_string()))),
        Effect::Delete => {
            backspace(1);
            let mut rest = text.to_string();
            rest.pop();
            kb.retitle(&rest);
        }
        Effect::Clear => backspace(text.chars().count()),
        _ => return false,
    }
    true
}

/// The text field that has focus this frame: filled in by `parts::text_field`, read by the app.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TvField {
    pub text: String,
    pub password: bool,
    pub hint: String,
    /// A name being typed (a playlist): the keyboard capitalises each word.
    pub name: bool,
}

fn field_id() -> Id {
    Id::new("tv-keyboard-field")
}

fn request_id() -> Id {
    Id::new("tv-keyboard-request")
}

/// FIX-2 #1d: ask the TV app to open the on-screen keyboard on the field that has focus next (the sign-in picker
/// does this for a user with a password, so the remote goes straight to typing it). The desktop ignores it.
pub fn request(ctx: &Context) {
    ctx.data_mut(|d| d.insert_temp(request_id(), true));
}

/// The pending open request, if any (cleared).
pub fn take_request(ctx: &Context) -> bool {
    ctx.data_mut(|d| {
        let r = d.get_temp::<bool>(request_id()).unwrap_or(false);
        d.remove::<bool>(request_id());
        r
    })
}

fn open_id() -> Id {
    Id::new("tv-keyboard-open")
}

/// FIX-4 (review 4): the app says each frame whether the TV keyboard is up, so a form can make room above it.
pub fn set_open(ctx: &Context, open: bool) {
    ctx.data_mut(|d| d.insert_temp(open_id(), open));
}

/// Whether the TV keyboard is up this frame (always false on the desktop).
pub fn is_open(ctx: &Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(open_id()).unwrap_or(false))
}

/// Hook for `parts::text_field`: this field has focus (call only when it does).
pub fn register(ctx: &Context, text: &str, password: bool, hint: &str) {
    let f = TvField { text: text.to_string(), password, hint: hint.to_string(), name: false };
    ctx.data_mut(|d| d.insert_temp(field_id(), f));
}

/// Mark the field registered this frame as a name field (K1). Call right after the field's `text_field`.
pub fn mark_name(ctx: &Context) {
    ctx.data_mut(|d| {
        if let Some(mut f) = d.get_temp::<TvField>(field_id()) {
            f.name = true;
            d.insert_temp(field_id(), f);
        }
    });
}

/// The field registered during this frame, if any (clears it: no field drawn next frame → none).
pub fn take_field(ctx: &Context) -> Option<TvField> {
    ctx.data_mut(|d| {
        let f = d.get_temp::<TvField>(field_id());
        d.remove::<TvField>(field_id());
        f
    })
}

/// Paint the keyboard over the bottom of the screen. Returns the key a click landed on (row, col).
pub fn draw(ctx: &Context, t: &Theme, st: &KeyboardState, f: &TvField) -> Option<(usize, usize)> {
    let screen = ctx.screen_rect();
    // FIX-2 #1c: a slimmer panel (shorter keys, narrower) so the first row of results above it stays in view.
    let key_h = t.osk_key_h;
    let (pad, key_gap, text_h) = (t.gap / 3.0, t.osk_gap, t.osk_text_h);
    let rows = st.layout();
    let rect = panel_rect(t, screen, rows.len());
    let w = rect.width();
    let cell = (w - 2.0 * pad) / (SIDE + 10.0);
    let mut clicked = None;
    egui::Area::new(Id::new("tv-keyboard")).order(egui::Order::Foreground).fixed_pos(rect.min).show(ctx, |ui| {
        ui.allocate_exact_size(rect.size(), Sense::hover());
        ui.painter().rect_filled(rect, t.radius * 2.0, t.surface);
        // The field's text so far (dots for a password), then a caret.
        // FIX-2 #1e: an empty field shows its hint dimmed, in the hint style — never as if it were typed text.
        let (shown, ink) = if f.text.is_empty() {
            (f.hint.clone(), t.muted.gamma_multiply(0.6))
        } else if f.password {
            ("•".repeat(f.text.chars().count()), t.text)
        } else {
            (f.text.clone(), t.text)
        };
        let g = ui.fonts(|fo| fo.layout_no_wrap(shown, t.font(t.osk_line_text), ink));
        let (tx, ty) = (rect.left() + pad, rect.top() + pad + (text_h - g.size().y) / 2.0);
        let end = tx + g.size().x;
        ui.painter().galley(pos2(tx, ty), g.clone(), ink);
        let caret_x = if f.text.is_empty() { tx } else { end + 2.0 };
        ui.painter().line_segment([pos2(caret_x, ty), pos2(caret_x, ty + g.size().y)], egui::Stroke::new(2.0, t.text));
        for (ri, row) in rows.iter().enumerate() {
            let mut x = rect.left() + pad;
            let y = rect.top() + pad + text_h + ri as f32 * (key_h + key_gap);
            for (ci, c) in row.iter().enumerate() {
                let kr = Rect::from_min_size(pos2(x, y), vec2(c.w * cell - key_gap, key_h));
                x += c.w * cell;
                let id = Id::new(("tv-kb-key", ri, ci));
                let resp = ui.interact(kr, id, Sense::click());
                let a = widgets::focus_amount(ui, t, id, st.row == ri && st.col == ci);
                let on = key_filled(st, c.cap);
                // K3: Shift on is a tinted plate with an outline, never the white plate of the focused key.
                let (rest, dim_ink, _) = key_look(t, false, on);
                let rest = if !on && resp.hovered() { t.glass_hi } else { rest };
                let plate_ink = widgets::plate(ui, t, kr, key_h / 2.0, rest, a, t.osk_focus_scale);
                let ink = if a > 0.5 { plate_ink } else { dim_ink };
                if on && a <= 0.5 {
                    ui.painter().rect_stroke(
                        kr,
                        key_h / 2.0,
                        egui::Stroke::new(on_look(t).1, t.text),
                        egui::StrokeKind::Inside,
                    );
                }
                // A long label ("SHIFT ON") shrinks to fit its key.
                let text = label(c.cap, st.symbols, st.shift);
                let lay =
                    |size: f32| ui.fonts(|fo| fo.layout_no_wrap(text.clone(), t.font_w(size, Weight::SemiBold), ink));
                let mut lg = lay(t.osk_key_text);
                let room = kr.width() - t.gap;
                if lg.size().x > room {
                    lg = lay(t.osk_key_text * room / lg.size().x);
                }
                ui.painter().galley(kr.center() - lg.size() / 2.0, lg, ink);
                if resp.clicked() {
                    clicked = Some((ri, ci));
                }
            }
        }
    });
    clicked
}

#[cfg(test)]
mod tests {
    use super::*;

    fn backspaces(ctx: &Context) -> usize {
        ctx.input(|i| {
            i.events.iter().filter(|e| matches!(e, egui::Event::Key { key: egui::Key::Backspace, .. })).count()
        })
    }

    #[test]
    fn a_click_on_delete_or_clear_types_like_the_remote() {
        let ctx = Context::default();
        let mut k = KeyboardState::for_field(&TvField { name: true, ..Default::default() });
        k.shift = false;
        // Delete: one backspace, and Shift comes back on when the text left ends in a space (a name field).
        assert!(apply_text(&ctx, &mut k, "Ab  ", Effect::Delete));
        assert_eq!(backspaces(&ctx), 1);
        assert!(k.shift);
        // Clear: one backspace per character.
        assert!(apply_text(&ctx, &mut k, "four", Effect::Clear));
        assert_eq!(backspaces(&ctx), 5);
        assert!(!apply_text(&ctx, &mut k, "x", Effect::Close));
    }

    fn at(row: usize, col: usize) -> KeyboardState {
        let mut k = KeyboardState::new();
        k.row = row;
        k.col = col;
        k.x = centre(&k.layout()[row], col);
        k
    }

    #[test]
    fn enter_types_the_focused_letter() {
        let mut k = KeyboardState::new();
        assert_eq!(k.press(Key::Enter), Effect::Type('a'));
        k.press(Key::Right);
        assert_eq!(k.press(Key::Enter), Effect::Type('b'));
    }

    #[test]
    fn arrows_move_and_stop_at_the_edges() {
        let mut k = KeyboardState::new();
        k.press(Key::Left);
        k.press(Key::Left);
        k.press(Key::Up);
        assert_eq!((k.row, k.col), (0, 0)); // the digit row's first key is the left edge
        for _ in 0..20 {
            k.press(Key::Right);
        }
        assert_eq!(k.col, 9);
        k.press(Key::Down);
        assert_eq!(k.row, 1);
        assert_eq!(k.press(Key::Enter), Effect::Type('j'));
    }

    #[test]
    fn down_keeps_to_the_same_screen_position_on_the_bottom_row() {
        // 'e' (col 4, centre 4.5) lands on 'delete'/'clear' side of the bottom row, never off the end.
        let mut k = at(3, 10);
        k.press(Key::Down);
        assert_eq!(k.row, 4);
        assert_eq!(k.focused_cap(), Cap::Done);
        k.press(Key::Up);
        assert_eq!((k.row, k.col), (3, 10)); // the way back lands on the same letter
        let mut k = at(3, 0);
        k.press(Key::Down);
        assert_eq!(k.focused_cap(), Cap::Space);
    }

    #[test]
    fn down_from_the_last_row_leaves_the_keyboard() {
        let mut k = at(4, 0);
        assert_eq!(k.press(Key::Down), Effect::Leave(Key::Down));
    }

    #[test]
    fn up_from_the_first_row_leaves() {
        // FIX-2 #1a: Up from the top row leaves to the screen (results / the field); from a lower row it moves up.
        let mut k = at(0, 3);
        assert_eq!(k.press(Key::Up), Effect::Leave(Key::Up));
        assert_eq!(k.row, 0);
        let mut k = at(1, 3);
        assert_eq!(k.press(Key::Up), Effect::None);
        assert_eq!(k.row, 0);
    }

    #[test]
    fn back_closes_and_delete_deletes() {
        let mut k = KeyboardState::new();
        assert_eq!(k.press(Key::Back), Effect::Close);
        assert_eq!(k.press(Key::Delete), Effect::Delete);
        assert_eq!(k.press(Key::Backspace), Effect::Delete);
    }

    #[test]
    fn bottom_row_keys() {
        let mut k = at(4, 0);
        assert_eq!(k.press(Key::Enter), Effect::Type(' '));
        k.press(Key::Right);
        assert_eq!(k.press(Key::Enter), Effect::Delete);
        k.press(Key::Right);
        assert_eq!(k.press(Key::Enter), Effect::Close);
        assert_eq!(at(3, 0).press(Key::Enter), Effect::Clear);
        assert_eq!(at(0, 0).press(Key::Enter), Effect::Type('1'));
    }

    #[test]
    fn shift_is_for_one_letter() {
        let mut k = at(1, 0);
        assert_eq!(k.focused_cap(), Cap::Shift);
        k.press(Key::Enter);
        assert!(k.shift);
        assert!(matches!(k.layout()[1][1].cap, Cap::Char('A')));
        let mut k2 = k.clone();
        k2.row = 1;
        k2.col = 1;
        assert_eq!(k2.press(Key::Enter), Effect::Type('A'));
        assert!(!k2.shift);
        assert_eq!(k2.press(Key::Enter), Effect::Type('a'));
    }

    #[test]
    fn the_123_layer_has_digits_and_address_symbols() {
        let mut k = at(2, 0);
        assert_eq!(k.focused_cap(), Cap::Layer);
        k.press(Key::Enter);
        assert!(k.symbols);
        let chars: Vec<char> =
            k.layout().iter().flatten().filter_map(|c| if let Cap::Char(c) = c.cap { Some(c) } else { None }).collect();
        for c in "0123456789:/.-_@".chars() {
            assert!(chars.contains(&c), "missing {c}");
        }
        k.row = 0;
        k.col = 0;
        assert_eq!(k.press(Key::Enter), Effect::Type('1'));
    }

    #[test]
    fn letters_layer_has_every_letter() {
        let chars: String = layout(false, false)
            .iter()
            .flatten()
            .filter_map(|c| if let Cap::Char(c) = c.cap { Some(c) } else { None })
            .collect();
        for c in 'a'..='z' {
            assert!(chars.contains(c), "missing {c}");
        }
    }

    #[test]
    fn quick_presses_never_skip_keys() {
        // FIX-2 #1b: eight quick Rights land on the eighth key (the old speed-up skipped keys: "landed on 't', one
        // key past 's'").
        let mut k = KeyboardState::new();
        let mut t = 0.0;
        for _ in 0..8 {
            k.press_at(Key::Right, Some(t));
            t += 0.03;
        }
        assert_eq!(k.col, 9);
    }

    #[test]
    fn plain_press_never_accelerates() {
        let mut k = KeyboardState::new();
        for _ in 0..8 {
            k.press(Key::Right);
        }
        assert_eq!(k.col, 9);
    }

    #[test]
    fn typed_text_keys_are_left_to_the_field() {
        // The physical keyboard's characters already reach egui as text; the model must not type them twice.
        assert_eq!(KeyboardState::new().press(Key::Text('x')), Effect::None);
    }
}
