//! FIX-3 #4 (E1–E4): the episode row scrolls every way, shows its end arrows without hover, and a new season starts
//! at its first episode.
use egui::{pos2, vec2, Event, Modifiers, MouseWheelUnit, Pos2, RawInput, Rect, Sense, Shape};
use jf_core::nav::{forget, move_focus, Focus, Group};
use jf_core::platform::Key;
use jf_core::theme::Theme;
use jf_core::ui::at_details as at;

const SCREEN: Rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(1200.0, 700.0));

struct Rig {
    ctx: egui::Context,
    t: Theme,
    cells: usize,
    pointer: Option<Pos2>,
    desktop: bool,
    time: f64,
}

impl Rig {
    fn new(cells: usize) -> Rig {
        Rig { ctx: egui::Context::default(), t: Theme::placeholder(), cells, pointer: None, desktop: true, time: 0.0 }
    }

    /// One frame; returns the painted shapes.
    fn frame(&mut self, events: Vec<Event>) -> Vec<Shape> {
        self.time += 0.1;
        let mut input = RawInput { screen_rect: Some(SCREEN), time: Some(self.time), events, ..Default::default() };
        if let Some(p) = self.pointer {
            input.events.insert(0, Event::PointerMoved(p));
        }
        let (t, cells, desktop) = (self.t.clone(), self.cells, self.desktop);
        at::set_desktop(&self.ctx, desktop);
        let out = self.ctx.run(input, |ctx| {
            egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx, |ui| {
                // inside the page's own scroller, as on the show page
                egui::ScrollArea::vertical()
                    .id_salt("page")
                    .auto_shrink([false, false])
                    .scroll_source(at::page_scroll_source(ui))
                    .show(ui, |ui| {
                        ui.add_space(100.0);
                        at::row(ui, &t, "test-row", cells, 120.0, None, |ui, _| {
                            ui.allocate_exact_size(vec2(220.0, 120.0), Sense::click()).1
                        });
                        ui.add_space(900.0);
                    });
            });
        });
        out.shapes.into_iter().map(|s| s.shape).collect()
    }

    fn settle(&mut self) -> Vec<Shape> {
        let mut last = vec![];
        for _ in 0..4 {
            last = self.frame(vec![]);
        }
        last
    }

    fn offset(&self) -> f32 {
        let key = egui::Id::new(("at-row-scroll", "test-row"));
        self.ctx.data_mut(|d| d.get_temp::<(f32, f32)>(key)).map(|s| s.0).unwrap_or(0.0)
    }

    fn arrows(&self, shapes: &[Shape]) -> Vec<Pos2> {
        let want = self.t.round_button / 2.0;
        let mut found = vec![];
        fn walk(s: &Shape, want: f32, found: &mut Vec<Pos2>) {
            match s {
                Shape::Circle(c) if (c.radius - want).abs() < 0.01 => found.push(c.center),
                Shape::Vec(v) => v.iter().for_each(|s| walk(s, want, found)),
                _ => {}
            }
        }
        shapes.iter().for_each(|s| walk(s, want, &mut found));
        found
    }
}

fn wheel(delta: egui::Vec2) -> Event {
    Event::MouseWheel { unit: MouseWheelUnit::Point, delta, modifiers: Modifiers::NONE }
}

#[test]
fn row_scrolls_on_horizontal_wheel() {
    let mut rig = Rig::new(12);
    rig.pointer = Some(pos2(600.0, 220.0));
    rig.settle();
    let before = rig.offset();
    for _ in 0..6 {
        rig.frame(vec![wheel(vec2(-80.0, 0.0))]);
    }
    rig.settle();
    assert!(rig.offset() > before + 50.0, "horizontal wheel moved the row: {} → {}", before, rig.offset());
    // and the vertical wheel still does
    let mid = rig.offset();
    for _ in 0..6 {
        rig.frame(vec![wheel(vec2(0.0, -80.0))]);
    }
    rig.settle();
    assert!(rig.offset() > mid + 50.0, "vertical wheel still moves it: {} → {}", mid, rig.offset());
}

#[test]
fn row_arrows_visible_when_overflowing_without_hover() {
    let mut rig = Rig::new(12);
    rig.pointer = None;
    let shapes = rig.settle();
    let a = rig.arrows(&shapes);
    assert_eq!(a.len(), 2, "both end arrows drawn at rest: {a:?}");
    assert!(a[0].x < 600.0 && a[1].x > 600.0, "one at each end: {a:?}");
}

#[test]
fn tv_keeps_its_look_without_arrows() {
    let mut rig = Rig::new(12);
    rig.desktop = false;
    let shapes = rig.settle();
    assert!(rig.arrows(&shapes).is_empty(), "no pointer on the TV: no arrows");
}

#[test]
fn no_arrows_without_overflow() {
    let mut rig = Rig::new(2);
    let shapes = rig.settle();
    assert!(rig.arrows(&shapes).is_empty(), "two cards fit: no arrows");
    rig.pointer = Some(pos2(600.0, 220.0));
    let shapes = rig.settle();
    assert!(rig.arrows(&shapes).is_empty(), "…not even under the pointer");
}

fn row(n: usize) -> Group {
    Group { len: n, cols: n, xs: (0..n).map(|i| i as f32 * 100.0).collect() }
}

#[test]
fn season_switch_enters_row_at_first_episode() {
    // tabs (2), episode row (8 cards) — Season 2 has Season 1's shape
    let g = [row(1), row(2), row(8), row(3)];
    let mut f = move_focus(&g, Focus { group: 1, index: 0 }, Key::Down);
    for _ in 0..5 {
        f = move_focus(&g, f, Key::Right);
    }
    assert_eq!(f, Focus { group: 2, index: 5 });
    let up = move_focus(&g, f, Key::Up);
    assert_eq!(up.group, 1);
    // without the switch, the remembered card comes back
    assert_eq!(move_focus(&g, up, Key::Down), Focus { group: 2, index: 5 });
    // a season switch forgets it
    forget(&g, 2);
    assert_eq!(move_focus(&g, up, Key::Down), Focus { group: 2, index: 0 });
}

#[test]
fn down_from_row_goes_to_next_section() {
    let g = [row(1), row(2), row(8), row(3)];
    let f = move_focus(&g, Focus { group: 2, index: 4 }, Key::Down);
    assert_eq!(f.group, 3, "Up Next / Cast follows the row");
}
