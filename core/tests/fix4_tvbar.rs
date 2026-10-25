//! FIX-4 tv-bar (FRICTION-5 #3): on TV only the focused thing is white. B1–B3 checked on what the pill bar draws
//! (text colours and the underline mark in one egui frame).
use egui::epaint::Shape;
use egui::Color32;
use jf_core::theme::Theme;
use jf_core::ui::parts::{pill_bar_themed, set_tv};

fn flat(s: &Shape, out: &mut Vec<Shape>) {
    match s {
        Shape::Vec(v) => v.iter().for_each(|s| flat(s, out)),
        s => out.push(s.clone()),
    }
}

struct Drawn {
    /// (label, colour) of each text drawn.
    texts: Vec<(String, Color32)>,
    /// Fill of each small underline mark (a filled rect thinner than 6 px).
    marks: Vec<Color32>,
}

fn draw(tv: bool, focused: Option<usize>) -> Drawn {
    let ctx = egui::Context::default();
    ctx.set_fonts(Theme::fonts());
    let t = Theme::placeholder();
    let mut shapes = vec![];
    // two frames so the focus animation has settled
    for _ in 0..2 {
        shapes.clear();
        let out = ctx.run(egui::RawInput::default(), |ctx| {
            set_tv(ctx, tv);
            egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx, |ui| {
                pill_bar_themed(ui, &t, &["Home", "Search", "Library"], 0, focused, None);
            });
        });
        out.shapes.iter().for_each(|c| flat(&c.shape, &mut shapes));
    }
    let mut d = Drawn { texts: vec![], marks: vec![] };
    for s in &shapes {
        match s {
            Shape::Text(tx) => d.texts.push((tx.galley.text().to_string(), tx.galley.job.sections[0].format.color)),
            Shape::Rect(r) if r.blur_width == 0.0 && r.rect.height() < 6.0 && r.fill != Color32::TRANSPARENT => {
                d.marks.push(r.fill)
            }
            _ => {}
        }
    }
    d
}

fn ink_of(d: &Drawn, label: &str) -> Color32 {
    d.texts.iter().find(|(l, _)| l == label).unwrap_or_else(|| panic!("no text {label}")).1
}

#[test]
fn current_tab_dimmed_when_bar_unfocused() {
    let t = Theme::placeholder();
    let dim = t.text.gamma_multiply(0.7);
    let d = draw(true, None);
    assert_eq!(ink_of(&d, "Home"), dim, "B1: the current tab is dimmed like the others");
    assert_eq!(ink_of(&d, "Search"), dim);
    // FIX-5 H3 made the unfocused TV underline fainter still (see fix5_herofocus.rs).
    assert_eq!(d.marks, [dim.gamma_multiply(0.5)], "B1: the underline is drawn dimmer than the dimmed ink");
}

#[test]
fn focused_tab_is_only_white_item() {
    let t = Theme::placeholder();
    let dim = t.text.gamma_multiply(0.7);
    let d = draw(true, Some(1));
    assert_ne!(ink_of(&d, "Search"), dim, "B2: the focused tab reads bright");
    assert_eq!(ink_of(&d, "Library"), dim);
    assert_eq!(ink_of(&d, "Home"), t.text, "B2: unchanged while the bar holds focus");
    assert_eq!(d.marks, [t.text], "B2: the current tab keeps its underline");
}

#[test]
fn desk_bar_unchanged() {
    let t = Theme::placeholder();
    let d = draw(false, None);
    assert_eq!(ink_of(&d, "Home"), t.text, "B3: desktop keeps the current tab full ink");
    assert_eq!(d.marks, [t.text]);
}
