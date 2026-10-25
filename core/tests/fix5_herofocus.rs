//! FIX-5 hero-focus (FRICTION-6 #1): on TV Home the cursor on Play reads at once. H1–H4 checked on what the hero
//! buttons and the pill bar draw (shapes in one egui frame).
use egui::epaint::Shape;
use egui::{Color32, Rect, StrokeKind};
use jf_core::theme::Theme;
use jf_core::ui::parts::{pill_bar_themed, set_tv};
use jf_core::ui::widgets::{hero_lift_room, hero_play_pill, play_pill_for, round_button, Icon};

fn flat(s: &Shape, out: &mut Vec<Shape>) {
    match s {
        Shape::Vec(v) => v.iter().for_each(|s| flat(s, out)),
        s => out.push(s.clone()),
    }
}

fn frames(tv: bool, mut add: impl FnMut(&mut egui::Ui)) -> Vec<Shape> {
    let ctx = egui::Context::default();
    ctx.set_fonts(Theme::fonts());
    let mut shapes = vec![];
    // two frames so the focus animation has settled
    for _ in 0..2 {
        shapes.clear();
        let out = ctx.run(egui::RawInput::default(), |ctx| {
            set_tv(ctx, tv);
            egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx, |ui| add(ui));
        });
        out.shapes.iter().for_each(|c| flat(&c.shape, &mut shapes));
    }
    shapes
}

/// The focus-ring rects (a stroked, unfilled rect drawn outside) in `shapes`.
fn rings(shapes: &[Shape]) -> Vec<(Rect, egui::Stroke, f32)> {
    shapes
        .iter()
        .filter_map(|s| match s {
            Shape::Rect(r)
                if r.fill == Color32::TRANSPARENT && r.stroke.width > 0.0 && r.stroke_kind == StrokeKind::Outside =>
            {
                Some((r.rect, r.stroke, r.corner_radius.nw as f32))
            }
            _ => None,
        })
        .collect()
}

/// The widest white plate (focus fill) rect.
fn plate_rect(shapes: &[Shape]) -> Option<Rect> {
    let t = Theme::placeholder();
    shapes.iter().find_map(|s| match s {
        Shape::Rect(r) if r.blur_width == 0.0 && r.fill == t.focus_fill => Some(r.rect),
        _ => None,
    })
}

fn hero_play(tv: bool, focused: bool) -> Vec<Shape> {
    let t = Theme::placeholder();
    frames(tv, |ui| {
        hero_play_pill(ui, &t, "Play", focused, tv);
    })
}

fn hero_more(tv: bool, focused: bool) -> Vec<Shape> {
    let t = Theme::placeholder();
    frames(tv, |ui| {
        round_button(ui, &t, Icon::More, focused);
    })
}

#[test]
fn hero_focused_button_has_ring_on_tv() {
    let t = Theme::placeholder();
    for (name, shapes) in [("Play", hero_play(true, true)), ("more", hero_more(true, true))] {
        let r = rings(&shapes);
        assert_eq!(r.len(), 1, "H1: {name} draws one ring when focused on TV");
        let (rect, stroke, radius) = r[0];
        assert_eq!(stroke.width, t.focus_stroke, "H1: {name} ring width is the card ring's");
        assert_eq!(stroke.color, t.focus_color, "H1: {name} ring colour is the card ring's");
        let plate = plate_rect(&shapes).expect("white plate");
        assert!((rect.height() - plate.height()).abs() < 0.5, "H1: ring follows the lifted plate");
        let plate_radius = shapes
            .iter()
            .find_map(|s| match s {
                Shape::Rect(r) if r.blur_width == 0.0 && r.fill == t.focus_fill => Some(r.corner_radius.nw as f32),
                _ => None,
            })
            .unwrap();
        assert_eq!(radius, plate_radius, "H1: ring follows the pill's radius");
    }
    assert!(rings(&hero_play(true, false)).is_empty(), "no ring on an unfocused TV Play");
    assert!(rings(&hero_more(true, false)).is_empty(), "no ring on an unfocused TV •••");
}

#[test]
fn hero_focus_lift_at_least_card_scale() {
    let t = Theme::placeholder();
    for (name, lit, base) in
        [("Play", hero_play(true, true), t.pill_h), ("more", hero_more(true, true), t.round_button)]
    {
        let h = plate_rect(&lit).expect("white plate").height();
        assert!(h / base >= t.focus_scale - 0.005, "H2: {name} lifted {} < card scale {}", h / base, t.focus_scale);
    }
}

fn bar(tv: bool, focused: Option<usize>) -> Vec<Shape> {
    let t = Theme::placeholder();
    frames(tv, |ui| {
        pill_bar_themed(ui, &t, &["Home", "Search", "Library"], 0, focused, None);
    })
}

/// (fill, height) of each small underline mark.
fn marks(shapes: &[Shape]) -> Vec<(Color32, f32)> {
    shapes
        .iter()
        .filter_map(|s| match s {
            Shape::Rect(r) if r.blur_width == 0.0 && r.rect.height() < 6.0 && r.fill != Color32::TRANSPARENT => {
                Some((r.fill, r.rect.height()))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn unfocused_bar_mark_dimmer_than_hero_ring() {
    let t = Theme::placeholder();
    let shapes = bar(true, None);
    let m = marks(&shapes);
    assert_eq!(m.len(), 1);
    let (fill, h) = m[0];
    let ring = t.focus_color;
    assert!(
        (fill.r() as f32) <= ring.r() as f32 * 0.5 && (fill.a() as f32) <= ring.a() as f32 * 0.5 + 0.5,
        "H3: mark {fill:?} not ≤ 50% of the ring {ring:?}"
    );
    assert!(h < t.badge_stroke * 2.0 - 0.1, "H3: mark thinner than today's ({h})");
    assert!(rings(&shapes).is_empty(), "H3: no ring on an unfocused bar");
    let plates = shapes
        .iter()
        .filter(|s| matches!(s, Shape::Rect(r) if r.blur_width == 0.0 && (r.fill == t.glass || r.fill == t.focus_fill)))
        .count();
    assert_eq!(plates, 0, "H3: no item of an unfocused bar draws a plate");
}

#[test]
fn bar_focused_unchanged() {
    let t = Theme::placeholder();
    let m = marks(&bar(true, Some(1)));
    assert_eq!(m, [(t.text, t.badge_stroke * 2.0)], "H4: with the bar focused the current tab keeps today's mark");
}

#[test]
fn desk_hero_and_bar_unchanged() {
    let t = Theme::placeholder();
    assert!(rings(&hero_play(false, true)).is_empty(), "desktop Play draws no ring");
    assert!(rings(&hero_more(false, true)).is_empty(), "desktop ••• draws no ring");
    let h = plate_rect(&hero_play(false, true)).unwrap().height();
    assert!((h / t.pill_h - t.pill_focus_scale).abs() < 0.005, "desktop keeps the shared pill lift");
    assert_eq!(marks(&bar(false, None)), [(t.text, t.badge_stroke * 2.0)], "desktop mark unchanged");
}

/// The hero row as `parts::hero` lays it out (Play, room for the lift, •••): the outer rect of each button's
/// focused look (ring included) when the cursor is on `focus`.
fn row_extents(focus: usize) -> (Rect, Rect) {
    let t = Theme::placeholder();
    let mut rects = vec![];
    frames(true, |ui| {
        rects.clear();
        ui.spacing_mut().item_spacing.x = t.gap / 2.0;
        ui.horizontal(|ui| {
            let p = hero_play_pill(ui, &t, "Resume from 0:29 of the long film", focus == 0, true);
            ui.add_space(hero_lift_room(&t, p.rect.width()));
            let m = round_button(ui, &t, Icon::More, focus == 1);
            rects.push(p.rect);
            rects.push(m.rect);
        });
    });
    let grow = |r: Rect, on: bool| {
        if on {
            Rect::from_center_size(r.center(), r.size() * t.focus_scale).expand(t.focus_stroke)
        } else {
            r
        }
    };
    (grow(rects[0], focus == 0), grow(rects[1], focus == 1))
}

#[test]
fn hero_lifted_ring_never_touches_neighbour() {
    for focus in [0, 1] {
        let (play, more) = row_extents(focus);
        assert!(!play.intersects(more), "focus {focus}: lifted+ringed {play:?} meets {more:?}");
    }
}

#[test]
fn film_page_and_poster_flow_play_keep_today_look() {
    let t = Theme::placeholder();
    let lit = frames(true, |ui| {
        play_pill_for(ui, &t, "Play", true, true);
    });
    assert!(rings(&lit).is_empty(), "play_pill_for (film page, Poster Flow) draws no ring");
    let h = plate_rect(&lit).unwrap().height();
    assert!((h / t.pill_h - t.pill_focus_scale).abs() < 0.005, "and keeps the shared pill lift");
}
