//! FIX-4 #8: a long drop-down shows there's more (D1 edge cues, D2 opens with context, D3 keys unchanged).
//! Every test drives the real lists (`widgets::dropdown` and the film page's `at_details::popover`) in an egui frame
//! and reads what they draw.
use egui::epaint::Shape;
use egui::{pos2, vec2, Rect};
use jf_core::platform::Key;
use jf_core::theme::Theme;
use jf_core::ui::at_details::{self, Note};
use jf_core::ui::widgets::{self, dropdown_key, DropdownEvent, DropdownState};

fn flat(s: &Shape, out: &mut Vec<Shape>) {
    match s {
        Shape::Vec(v) => v.iter().for_each(|s| flat(s, out)),
        s => out.push(s.clone()),
    }
}

/// (edge fades drawn, chevrons drawn) in one frame's output.
fn cues(shapes: &[egui::epaint::ClippedShape]) -> (usize, usize) {
    let mut v = vec![];
    shapes.iter().for_each(|c| flat(&c.shape, &mut v));
    let fades =
        v.iter().filter(|s| matches!(s, Shape::Mesh(m) if m.vertices.len() == 4 && m.indices.len() == 6)).count();
    let chevrons = v.iter().filter(|s| matches!(s, Shape::Path(p) if p.points.len() == 3 && !p.closed)).count();
    (fades, chevrons)
}

fn labels(n: usize) -> Vec<String> {
    (0..n).map(|i| if i == 0 { "Off".to_string() } else { format!("Language {i}") }).collect()
}

fn new_ctx() -> egui::Context {
    let ctx = egui::Context::default();
    ctx.set_fonts(Theme::fonts());
    ctx
}

fn raw() -> egui::RawInput {
    egui::RawInput { screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1280.0, 800.0))), ..Default::default() }
}

/// Run the film page's popover for `frames` frames; returns the last frame's (row rects, cues).
fn popover_frames(
    ctx: &egui::Context,
    n: usize,
    focus: impl Fn(usize) -> Option<usize>,
    frames: usize,
) -> (Vec<Rect>, (usize, usize)) {
    let t = Theme::placeholder();
    let l = labels(n);
    let mut last = (vec![], (0, 0));
    for f in 0..frames {
        let mut rects = vec![];
        let out = ctx.run(raw(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let anchor = Rect::from_min_size(pos2(200.0, 100.0), vec2(160.0, 48.0));
                rects =
                    at_details::popover(ui, &t, "t", anchor, &l, focus(f), Note::None).iter().map(|r| r.rect).collect();
            });
        });
        last = (rects, cues(&out.shapes));
    }
    last
}

/// How far the popover list has scrolled, from where its first row sits against an unscrolled list.
fn popover_offset(ctx: &egui::Context, n: usize, focus: impl Fn(usize) -> Option<usize>, frames: usize) -> f32 {
    let base = popover_frames(&new_ctx(), n, |_| None, 4).0[0].top();
    base - popover_frames(ctx, n, focus, frames).0[0].top()
}

/// A drop-down opened on `highlighted`, run for a few frames; returns the cues on the last frame.
fn dropdown_cues(n: usize, highlighted: usize) -> (usize, usize) {
    let ctx = new_ctx();
    let t = Theme::placeholder();
    let opts = labels(n);
    let mut state = DropdownState { open: true, highlighted };
    let mut seen = (0, 0);
    for _ in 0..5 {
        let out = ctx.run(raw(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                widgets::dropdown(ui, &t, &mut state, "Subtitles", &opts, true);
            });
        });
        seen = cues(&out.shapes);
    }
    // the button's own chevron is not an edge cue
    (seen.0, seen.1 - 1)
}

#[test]
fn cue_shown_only_when_rows_hidden_above() {
    // selection deep in a long list: it opens scrolled, so rows are hidden above -> a fade and chevron on top
    let (fades, chevrons) = dropdown_cues(20, 12);
    assert!(fades >= 2 && chevrons >= 2, "rows hidden above and below: both edges cued, got {fades}/{chevrons}");
    // selection at the end: only the top edge is cued
    assert_eq!(dropdown_cues(20, 19), (1, 1), "scrolled to the end: cue above only");
    // film page popover opened on a deep row
    let (_, c) = popover_frames(&new_ctx(), 20, |_| Some(12), 5);
    assert!(c.0 >= 2, "popover deep selection: both edges cued, got {c:?}");
}

#[test]
fn cue_shown_only_when_rows_hidden_below() {
    assert_eq!(dropdown_cues(20, 0), (1, 1), "top of a long list: cue below only");
    let (_, c) = popover_frames(&new_ctx(), 20, |_| Some(0), 5);
    assert_eq!(c, (1, 1), "popover at the top: cue below only");
    let (_, c) = popover_frames(&new_ctx(), 20, |_| Some(19), 5);
    assert_eq!(c, (1, 1), "popover opened on the last row: cue above only");
}

#[test]
fn opens_with_selected_and_context_in_view() {
    let t = Theme::placeholder();
    let row_h = t.button_h + t.gap / 4.0;
    let ctx = new_ctx();
    let (rows, _) = popover_frames(&ctx, 20, |_| Some(12), 5);
    let off = popover_offset(&new_ctx(), 20, |_| Some(12), 5);
    assert!(off > 0.0, "a deep selection opens scrolled, offset {off}");
    // the view is the first rows' span: the selected row is the last one in it and fills the bottom
    let view_h = t.menu_max_h;
    let top = rows[12].top() - rows[0].top();
    assert!(top - off >= 0.0 && top - off + row_h <= view_h + 1.0, "row 12 in view: top {top} off {off}");
    // as many rows above it as fit: the row above the selected one is in view too, and so is row 12 - (fit - 1)
    let fit = (view_h / row_h).floor() as usize;
    let first_visible = 12 + 1 - fit;
    assert!(rows[first_visible].top() - rows[0].top() - off >= -1.0, "row {first_visible} in view");
    // the drop-down widget opens the same way (selection mid-list -> rows hidden above)
    assert!(dropdown_cues(20, 12).0 >= 2);
    // a selection near the top keeps "Off" visible: nothing scrolls
    assert!(popover_offset(&new_ctx(), 20, |_| Some(2), 5).abs() < 0.5);
}

#[test]
fn short_list_no_scroll() {
    assert_eq!(dropdown_cues(3, 2), (0, 0));
    assert_eq!(dropdown_cues(3, 0), (0, 0));
    for sel in 0..3 {
        let (_, c) = popover_frames(&new_ctx(), 3, |_| Some(sel), 5);
        assert_eq!(c, (0, 0), "short popover shows no cue (selection {sel})");
        assert!(popover_offset(&new_ctx(), 3, |_| Some(sel), 5).abs() < 0.5);
    }
}

#[test]
fn key_edges_unchanged() {
    let s = DropdownState { open: true, highlighted: 0 };
    let (s, ev) = dropdown_key(s, Key::Up, 10);
    assert_eq!((s.highlighted, ev), (0, None), "Up from the first row stays (no wrap)");
    let s = DropdownState { open: true, highlighted: 9 };
    let (s, ev) = dropdown_key(s, Key::Down, 10);
    assert_eq!((s.highlighted, ev), (9, None), "Down from the last row stays (no wrap)");
    let (s, ev) = dropdown_key(s, Key::Enter, 10);
    assert_eq!((s.open, ev), (false, Some(DropdownEvent::Pick(9))));
}
