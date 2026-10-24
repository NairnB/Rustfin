//! FIX-3 (C3-TV-CARD-FOCUS) rules that need no server: one clear focus look for every card on TV (FRICTION-4 #3).
use jf_core::ui::parts::{card_focus_style, CardFocusStyle};

/// C1: on TV a focused card has a solid white ring as well as the lift and glow.
#[test]
fn tv_focused_card_has_white_ring() {
    let s = card_focus_style(true, true, false);
    assert!(s.lift && s.glow && s.ring, "{s:?}");
    // The ring is for focus (the remote); a pointer hovering on a TV build is not a TV user.
    assert!(!card_focus_style(true, false, false).ring);
}

/// C2: on TV focus never lowers the poster's contrast — no sheen over a focused card.
#[test]
fn tv_focus_has_no_sheen() {
    for (focused, hovered) in [(true, false), (false, true), (true, true), (false, false)] {
        assert!(!card_focus_style(true, focused, hovered).sheen, "focused={focused} hovered={hovered}");
    }
}

/// C4: desktop hover and focus are unchanged: lift + glow + sheen, no ring.
#[test]
fn desk_card_focus_unchanged() {
    let on = CardFocusStyle { lift: true, glow: true, ring: false, sheen: true };
    assert_eq!(card_focus_style(false, true, false), on);
    assert_eq!(card_focus_style(false, false, true), on);
    assert_eq!(card_focus_style(false, true, true), on);
    let off = CardFocusStyle { lift: false, glow: false, ring: false, sheen: false };
    assert_eq!(card_focus_style(false, false, false), off);
}

// ---- the real drawing decisions: run the card painters in an egui frame and look at the shapes they emit ----
use egui::epaint::Shape;
use jf_core::theme::Theme;
use jf_core::ui::{at_details, parts};

fn flat(s: &Shape, out: &mut Vec<Shape>) {
    match s {
        Shape::Vec(v) => v.iter().for_each(|s| flat(s, out)),
        s => out.push(s.clone()),
    }
}

/// Shapes drawn by `draw` in one frame, with the TV flag set as given.
fn shapes(tv: bool, draw: impl FnOnce(&mut egui::Ui, &Theme)) -> Vec<Shape> {
    let ctx = egui::Context::default();
    ctx.set_fonts(Theme::fonts());
    let t = Theme::placeholder();
    let mut draw = Some(draw);
    let out = ctx.run(egui::RawInput::default(), |ctx| {
        parts::set_tv(ctx, tv);
        egui::CentralPanel::default().show(ctx, |ui| (draw.take().unwrap())(ui, &t));
    });
    let mut v = vec![];
    out.shapes.iter().for_each(|c| flat(&c.shape, &mut v));
    v
}

struct Seen {
    glow: usize,
    ring: usize,
    sheen: usize,
}

fn seen(v: &[Shape], t: &Theme) -> Seen {
    let rects = v.iter().filter_map(|s| if let Shape::Rect(r) = s { Some(r) } else { None });
    let (mut glow, mut ring) = (0, 0);
    for r in rects {
        if r.blur_width > 0.0 && r.fill.a() > 0 && r.fill.r() == t.focus_glow.r() && r.fill.a() <= t.focus_glow.a() {
            glow += 1;
        }
        if (r.stroke.width - t.focus_stroke).abs() < 0.01 && r.fill == egui::Color32::TRANSPARENT {
            ring += 1;
        }
    }
    let sheen = v.iter().filter(|s| matches!(s, Shape::Mesh(_))).count();
    Seen { glow, ring, sheen }
}

fn poster(focused: bool) -> impl FnOnce(&mut egui::Ui, &Theme) {
    move |ui, t| {
        at_details::poster_card(ui, t, None, "Poster", focused);
    }
}

#[test]
fn drawn_poster_on_tv_has_glow_ring_no_sheen() {
    let t = Theme::placeholder();
    let s = seen(&shapes(true, poster(true)), &t);
    assert_eq!((s.glow, s.ring, s.sheen), (1, 1, 0));
    let s = seen(&shapes(true, poster(false)), &t);
    assert_eq!((s.glow, s.ring, s.sheen), (0, 0, 0));
}

/// C4: the details-page art on desktop looks as it did before — no glow, no ring, the sheen stays.
#[test]
fn drawn_poster_on_desktop_has_no_glow_or_ring() {
    let t = Theme::placeholder();
    let s = seen(&shapes(false, poster(true)), &t);
    assert_eq!((s.glow, s.ring), (0, 0));
    assert!(s.sheen > 0, "desktop keeps the sheen");
}

#[test]
fn drawn_card_lift_follows_the_style() {
    let t = Theme::placeholder();
    let lift = |ui: &mut egui::Ui, t: &Theme| {
        parts::card_lift(ui, t, egui::Rect::from_min_size(egui::pos2(100.0, 100.0), egui::vec2(200.0, 300.0)), 1.0);
    };
    let tv = seen(&shapes(true, lift), &t);
    assert_eq!((tv.glow, tv.ring), (1, 1));
    let desk = seen(&shapes(false, lift), &t);
    assert_eq!((desk.glow, desk.ring), (1, 0), "desktop card: glow, no ring");
}

/// C3: on TV a focused person-page / gallery poster draws exactly what a focused grid card draws (glow + ring, no sheen).
#[test]
fn poster_card_focus_matches_card() {
    let t = Theme::placeholder();
    let lift = |ui: &mut egui::Ui, t: &Theme| {
        parts::card_lift(ui, t, egui::Rect::from_min_size(egui::pos2(100.0, 100.0), egui::vec2(200.0, 300.0)), 1.0);
    };
    let card = seen(&shapes(true, lift), &t);
    let poster = seen(&shapes(true, poster(true)), &t);
    assert_eq!((poster.glow, poster.ring), (card.glow, card.ring));
    assert_eq!(poster.sheen, 0, "and no sheen, as `parts::card` on TV");
    assert!(!card_focus_style(true, true, false).sheen);
}

/// Pointer hover on a TV build never lights a card (no ring from the mouse); on desktop hover still does.
#[test]
fn tv_pointer_hover_never_lights_a_card() {
    let ctx = egui::Context::default();
    parts::set_tv(&ctx, true);
    assert!(!parts::card_lit(&ctx, false, true), "TV: hover alone");
    assert!(parts::card_lit(&ctx, true, false), "TV: remote focus");
    parts::set_tv(&ctx, false);
    assert!(parts::card_lit(&ctx, false, true), "desktop: hover lights the card as before");
    assert!(parts::card_lit(&ctx, true, false));
    assert!(!parts::card_lit(&ctx, false, false));
}

/// Every card painter in `parts` (grid `card`, Archive `thumb_card`) decides "lit" through `card_lit`, so none can
/// ring on pointer hover on TV. Fails if a painter goes back to its own `focused || resp.hovered()`.
#[test]
fn every_card_painter_asks_card_lit() {
    let src = include_str!("../src/ui/parts.rs");
    for name in ["pub fn card(", "pub fn thumb_card("] {
        let start = src.find(name).unwrap_or_else(|| panic!("{name} not found"));
        let body = &src[start..];
        let body = &body[..body[1..].find("\npub fn ").map_or(body.len(), |i| i + 1)];
        assert!(body.contains("card_lit(ui.ctx(), focused, resp.hovered())"), "{name} must use card_lit");
        assert!(!body.contains("|| resp.hovered()"), "{name} lights on hover by itself");
    }
}

/// Cast circles on the details page get the white ring on TV when focused, and not on desktop.
#[test]
fn cast_circle_rings_on_tv_only() {
    let t = Theme::placeholder();
    let rings = |tv: bool, focused: bool| {
        let v = shapes(tv, move |ui, t| {
            at_details::person_circle(ui, t, None, "Rowan Pike", "Lead", focused);
        });
        v.iter()
            .filter(|s| {
                matches!(s, Shape::Circle(c) if (c.stroke.width - t.focus_stroke).abs() < 0.01
                && c.stroke.color.a() > 0 && c.stroke.color.r() == t.focus_color.r())
            })
            .count()
    };
    assert_eq!(rings(true, true), 1, "TV focused");
    assert_eq!(rings(true, false), 0, "TV not focused");
    assert_eq!(rings(false, true), 0, "desktop focused: as on main");
}
