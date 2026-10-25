//! FIX-3 tv-hero-focus (FRICTION-4 #2): on TV only the focused button is white. H1, H3, H4 checked on what the Play
//! pill actually draws (its plate's fill in one egui frame).
use egui::epaint::Shape;
use egui::Color32;
use jf_core::theme::Theme;
use jf_core::ui::parts::set_tv;
use jf_core::ui::widgets::play_pill_for;

fn flat(s: &Shape, out: &mut Vec<Shape>) {
    match s {
        Shape::Vec(v) => v.iter().for_each(|s| flat(s, out)),
        s => out.push(s.clone()),
    }
}

/// The fills of the filled rects the Play pill draws in one frame (`tv` = the TV build's pill).
fn plate_fills(tv: bool, focused: bool) -> Vec<Color32> {
    let ctx = egui::Context::default();
    ctx.set_fonts(Theme::fonts());
    let t = Theme::placeholder();
    let out = ctx.run(egui::RawInput::default(), |ctx| {
        set_tv(ctx, tv);
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx, |ui| {
            play_pill_for(ui, &t, "Play", focused, tv);
        });
    });
    let mut v = vec![];
    out.shapes.iter().for_each(|c| flat(&c.shape, &mut v));
    v.iter()
        .filter_map(|s| match s {
            Shape::Rect(r) if r.blur_width == 0.0 && r.fill != Color32::TRANSPARENT => Some(r.fill),
            _ => None,
        })
        .collect()
}

#[test]
fn tv_play_pill_rests_as_glass() {
    let t = Theme::placeholder();
    assert_eq!(plate_fills(true, false), [t.glass], "an unfocused TV Play is the translucent plate, not white");
}

#[test]
fn tv_play_pill_turns_white_when_focused() {
    let t = Theme::placeholder();
    assert_eq!(plate_fills(true, true), [t.focus_fill]);
}

#[test]
fn desk_play_pill_unchanged() {
    let t = Theme::placeholder();
    assert_eq!(plate_fills(false, false), [t.focus_fill.gamma_multiply(0.85)], "desktop Play rests white");
    assert_eq!(plate_fills(false, true), [t.focus_fill]);
}

/// C3-POLISH #2: the flipped Poster Flow card's Play follows the same rule (it says whether this is the TV build).
#[test]
fn flipped_poster_flow_play_is_drawn_for_the_tv() {
    let src = include_str!("../src/ui/posterflow.rs");
    assert!(src.contains("widgets::play_pill_for(ui, t, label, focused, parts::is_tv(ui.ctx()))"));
    assert!(!src.contains("widgets::play_pill(ui"), "the desktop-only pill would rest white on TV");
}

/// C3-POLISH #9: on TV the mouse never moves focus. Every hover → focus line in the person page and the library
/// browser is gated on `hover_moves_focus` (fails if one goes back to plain `hovered()`).
#[test]
fn tv_hover_does_not_move_focus() {
    let ctx = egui::Context::default();
    assert!(jf_core::ui::parts::hover_moves_focus(&ctx));
    set_tv(&ctx, true);
    assert!(!jf_core::ui::parts::hover_moves_focus(&ctx));
    for (name, src) in
        [("person.rs", include_str!("../src/ui/person.rs")), ("browse.rs", include_str!("../src/ui/browse.rs"))]
    {
        let lines: Vec<&str> = src.lines().collect();
        let mut gated = 0;
        for w in lines.windows(2) {
            if w[0].trim_start().starts_with("if ") && w[0].contains(".hovered()") && w[1].contains("self.focus = ") {
                assert!(w[0].contains("hover_moves_focus"), "{name}: hover moves focus on TV: {}", w[0].trim());
                gated += 1;
            }
        }
        assert!(gated > 0, "{name}: no hover → focus line found (the check would pass vacuously)");
    }
}
