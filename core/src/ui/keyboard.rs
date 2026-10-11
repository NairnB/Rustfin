//! UXP5: the TV's on-screen keyboard — a tvOS-style grid (3 rows of keys, then space / delete / clear / shift /
//! 123 / done). The model (`KeyboardState::press_at` → `Effect`) is pure and unit-tested; `draw` paints it.
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
    /// Down from the last row: close the keyboard and give this key to the screen (focus moves on).
    Leave(Key),
}

const LETTERS: [&str; 3] = ["abcdefghij", "klmnopqrst", "uvwxyz.-_@"];
const SYMBOLS: [&str; 3] = ["1234567890", ":/.-_@?&=+", "!#$%^*()~,"];
/// Held arrows: a repeat inside this many seconds counts as "held".
const HELD_SECS: f64 = 0.2;

pub fn layout(symbols: bool, shift: bool) -> Vec<Vec<Cell>> {
    let src = if symbols { &SYMBOLS } else { &LETTERS };
    let mut rows: Vec<Vec<Cell>> = src
        .iter()
        .map(|r| {
            r.chars()
                .map(|c| Cell { cap: Cap::Char(if shift && !symbols { c.to_ascii_uppercase() } else { c }), w: 1.0 })
                .collect()
        })
        .collect();
    let bottom = [
        (Cap::Space, 2.5),
        (Cap::Delete, 1.5),
        (Cap::Clear, 1.5),
        (Cap::Shift, 1.5),
        (Cap::Layer, 1.5),
        (Cap::Done, 1.5),
    ];
    rows.push(bottom.iter().map(|&(cap, w)| Cell { cap, w }).collect());
    rows
}

pub fn label(cap: Cap, symbols: bool) -> String {
    match cap {
        Cap::Char(c) => c.to_string(),
        Cap::Space => "space".into(),
        Cap::Delete => "delete".into(),
        Cap::Clear => "clear".into(),
        Cap::Shift => "shift".into(),
        Cap::Layer => if symbols { "abc" } else { "123" }.into(),
        Cap::Done => "done".into(),
    }
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
    /// Where (in cells) the focus was last moved sideways: Up/Down keep to it across rows of different shapes.
    x: f32,
    /// The arrow held, when it was last seen, and how many repeats in a row.
    held: Option<(Key, f64, u32)>,
}

impl Default for KeyboardState {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyboardState {
    pub fn new() -> Self {
        KeyboardState { row: 0, col: 0, shift: false, symbols: false, x: 0.5, held: None }
    }

    pub fn layout(&self) -> Vec<Vec<Cell>> {
        layout(self.symbols, self.shift)
    }

    pub fn focused_cap(&self) -> Cap {
        self.layout()[self.row][self.col].cap
    }

    /// `press_at` without a clock (no held-key acceleration).
    pub fn press(&mut self, key: Key) -> Effect {
        self.press_at(key, None)
    }

    /// One key from the remote. `now` (seconds) lets held arrows speed up: after a few repeats Left/Right skip keys.
    pub fn press_at(&mut self, key: Key, now: Option<f64>) -> Effect {
        let rows = self.layout();
        let steps = self.steps(key, now);
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
                }
                Effect::None
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
            Cap::Space => Effect::Type(' '),
            Cap::Delete => Effect::Delete,
            Cap::Clear => Effect::Clear,
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

    /// How far a Left/Right moves: 1, then 2 and 3 as the arrow is held.
    fn steps(&mut self, key: Key, now: Option<f64>) -> usize {
        let arrow = matches!(key, Key::Left | Key::Right);
        let (Some(now), true) = (now, arrow) else {
            self.held = None;
            return 1;
        };
        let n = match self.held {
            Some((k, at, n)) if k == key && now - at <= HELD_SECS => n + 1,
            _ => 0,
        };
        self.held = Some((key, now, n));
        match n {
            0..=3 => 1,
            4..=9 => 2,
            _ => 3,
        }
    }
}

/// The text field that has focus this frame: filled in by `parts::text_field`, read by the app.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TvField {
    pub text: String,
    pub password: bool,
    pub hint: String,
}

fn field_id() -> Id {
    Id::new("tv-keyboard-field")
}

/// Hook for `parts::text_field`: this field has focus (call only when it does).
pub fn register(ctx: &Context, text: &str, password: bool, hint: &str) {
    let f = TvField { text: text.to_string(), password, hint: hint.to_string() };
    ctx.data_mut(|d| d.insert_temp(field_id(), f));
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
    let w = (screen.width() - 2.0 * t.margin).min(1100.0);
    let (pad, key_gap, text_h) = (t.gap / 2.0, 8.0, 56.0);
    let rows = st.layout();
    let h = 2.0 * pad + text_h + rows.len() as f32 * (t.button_h + key_gap);
    let rect = Rect::from_min_size(pos2(screen.center().x - w / 2.0, screen.bottom() - h), vec2(w, h));
    let cell = (w - 2.0 * pad) / 10.0;
    let mut clicked = None;
    egui::Area::new(Id::new("tv-keyboard")).order(egui::Order::Foreground).fixed_pos(rect.min).show(ctx, |ui| {
        ui.allocate_exact_size(rect.size(), Sense::hover());
        // Extends below the window so only the top corners show as rounded.
        let r = t.radius * 2.0;
        ui.painter().rect_filled(Rect::from_min_max(rect.min, rect.max + vec2(0.0, r)), r, t.surface);
        // The field's text so far (dots for a password), then a caret.
        let (shown, ink) = if f.text.is_empty() {
            (f.hint.clone(), t.muted)
        } else if f.password {
            ("•".repeat(f.text.chars().count()), t.text)
        } else {
            (f.text.clone(), t.text)
        };
        let g = ui.fonts(|fo| fo.layout_no_wrap(shown, t.font(t.body_size * 1.4), ink));
        let (tx, ty) = (rect.left() + pad, rect.top() + pad + (text_h - g.size().y) / 2.0);
        let end = tx + g.size().x;
        ui.painter().galley(pos2(tx, ty), g.clone(), ink);
        let caret_x = if f.text.is_empty() { tx } else { end + 2.0 };
        ui.painter().line_segment([pos2(caret_x, ty), pos2(caret_x, ty + g.size().y)], egui::Stroke::new(2.0, t.text));
        for (ri, row) in rows.iter().enumerate() {
            let mut x = rect.left() + pad;
            let y = rect.top() + pad + text_h + ri as f32 * (t.button_h + key_gap);
            for (ci, c) in row.iter().enumerate() {
                let kr = Rect::from_min_size(pos2(x, y), vec2(c.w * cell - key_gap, t.button_h));
                x += c.w * cell;
                let id = Id::new(("tv-kb-key", ri, ci));
                let resp = ui.interact(kr, id, Sense::click());
                let a = widgets::focus_amount(ui, t, id, st.row == ri && st.col == ci);
                let on = c.cap == Cap::Shift && st.shift;
                let rest = if on || resp.hovered() { t.glass_hi } else { t.glass };
                let ink = widgets::plate(ui, t, kr, t.button_h / 2.0, rest, a, t.pill_focus_scale);
                let lg = ui.fonts(|fo| {
                    fo.layout_no_wrap(label(c.cap, st.symbols), t.font_w(t.body_size, Weight::SemiBold), ink)
                });
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
        k.press(Key::Up);
        assert_eq!((k.row, k.col), (0, 0));
        for _ in 0..20 {
            k.press(Key::Right);
        }
        assert_eq!(k.col, 9);
        k.press(Key::Down);
        assert_eq!(k.row, 1);
        assert_eq!(k.press(Key::Enter), Effect::Type('t'));
    }

    #[test]
    fn down_keeps_to_the_same_screen_position_on_the_bottom_row() {
        // 'e' (col 4, centre 4.5) lands on 'delete'/'clear' side of the bottom row, never off the end.
        let mut k = at(2, 9);
        k.press(Key::Down);
        assert_eq!(k.row, 3);
        assert_eq!(k.focused_cap(), Cap::Done);
        k.press(Key::Up);
        assert_eq!((k.row, k.col), (2, 9)); // the way back lands on the same letter
        let mut k = at(2, 0);
        k.press(Key::Down);
        assert_eq!(k.focused_cap(), Cap::Space);
    }

    #[test]
    fn down_from_the_last_row_leaves_the_keyboard() {
        let mut k = at(3, 0);
        assert_eq!(k.press(Key::Down), Effect::Leave(Key::Down));
    }

    #[test]
    fn up_from_the_first_row_stays() {
        let mut k = KeyboardState::new();
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
        let mut k = at(3, 0);
        assert_eq!(k.press(Key::Enter), Effect::Type(' '));
        k.press(Key::Right);
        assert_eq!(k.press(Key::Enter), Effect::Delete);
        k.press(Key::Right);
        assert_eq!(k.press(Key::Enter), Effect::Clear);
        k.press(Key::Right);
        k.press(Key::Right);
        k.press(Key::Right);
        assert_eq!(k.press(Key::Enter), Effect::Close);
    }

    #[test]
    fn shift_is_for_one_letter() {
        let mut k = at(3, 3);
        assert_eq!(k.focused_cap(), Cap::Shift);
        k.press(Key::Enter);
        assert!(k.shift);
        assert!(matches!(k.layout()[0][0].cap, Cap::Char('A')));
        let mut k2 = k.clone();
        k2.row = 0;
        k2.col = 0;
        assert_eq!(k2.press(Key::Enter), Effect::Type('A'));
        assert!(!k2.shift);
        assert_eq!(k2.press(Key::Enter), Effect::Type('a'));
    }

    #[test]
    fn the_123_layer_has_digits_and_address_symbols() {
        let mut k = at(3, 4);
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
    fn held_arrow_speeds_up_and_a_pause_resets() {
        let mut k = KeyboardState::new();
        let mut t = 0.0;
        for _ in 0..4 {
            k.press_at(Key::Right, Some(t));
            t += 0.05;
        }
        assert_eq!(k.col, 4); // four quick presses: still one key each
        k.press_at(Key::Right, Some(t));
        assert_eq!(k.col, 6); // held on: two keys per press
        let mut k = KeyboardState::new();
        k.press_at(Key::Right, Some(0.0));
        k.press_at(Key::Right, Some(1.0)); // a pause: back to single steps
        assert_eq!(k.col, 2);
    }

    #[test]
    fn plain_press_never_accelerates() {
        let mut k = KeyboardState::new();
        for _ in 0..8 {
            k.press(Key::Right);
        }
        assert_eq!(k.col, 8);
    }

    #[test]
    fn typed_text_keys_are_left_to_the_field() {
        // The physical keyboard's characters already reach egui as text; the model must not type them twice.
        assert_eq!(KeyboardState::new().press(Key::Text('x')), Effect::None);
    }
}
