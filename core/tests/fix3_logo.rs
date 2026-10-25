//! FIX-3 logo-glow rules: a title logo is sized by its visible pixels (L1, L3), a dark one gets a thin halo hugging the glyphs (L2), a light one none (L4).
use egui::{pos2, vec2, Rect};
use jf_core::ui::at_details::{
    logo_choice, logo_fit, logo_halo_image, logo_info, logo_layout, LogoInfo, LOGO_HALO_ALPHA, LOGO_HALO_PAD,
    LOGO_MAX_UPSCALE, LOGO_WIDE_WAIT,
};

/// A 400 × 200 PNG that is transparent except a `w × h` block at (x, y).
fn padded(rgba: [u8; 4], x: u32, y: u32, w: u32, h: u32) -> Vec<u8> {
    let mut img = image::RgbaImage::from_pixel(400, 200, image::Rgba([0, 0, 0, 0]));
    for j in y..y + h {
        for i in x..x + w {
            img.put_pixel(i, j, image::Rgba(rgba));
        }
    }
    let mut out = std::io::Cursor::new(vec![]);
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

#[test]
fn logo_bounds_find_visible_pixels() {
    let info = logo_info(&padded([10, 10, 10, 255], 100, 80, 200, 40));
    assert_eq!(info.bounds, Rect::from_min_max(pos2(0.25, 0.4), pos2(0.75, 0.6)));
    assert!(info.dark);
    let faint = logo_info(&padded([255, 255, 255, 10], 100, 80, 200, 40));
    assert_eq!(faint.bounds, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), "alpha under ~10 % is not visible");
    let none = logo_info(b"not an image");
    assert_eq!(none.bounds, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)));
    assert!(!none.dark);
    // undecodable bytes are not "ok" (a wide fetch that fetched but cannot be read must not replace the narrow logo)
    assert!(!none.ok);
    assert!(info.ok);
}

#[test]
fn padded_logo_fills_the_title_box() {
    // 160 × 40 visible of 400 × 200: fills the 560 × 180 box's width at 3.5× → capped at 2×; never shrunk below the full fit
    let bytes = padded([255, 255, 255, 255], 120, 80, 160, 40);
    let info = logo_info(&bytes);
    let size = logo_fit(vec2(400.0, 200.0), info.bounds, vec2(560.0, 180.0), 1.0);
    assert!((size - vec2(320.0, 80.0)).length() < 0.01, "visible part, up-scaled by the 2× cap: {size:?}");
    let full =
        logo_fit(vec2(400.0, 200.0), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), vec2(560.0, 180.0), 1.0);
    assert!(size.x > full.x * 0.5 && LOGO_MAX_UPSCALE == 2.0);
    // a less padded logo fills one axis of the box
    let info = logo_info(&padded([255, 255, 255, 255], 20, 20, 360, 160));
    let size = logo_fit(vec2(400.0, 200.0), info.bounds, vec2(560.0, 180.0), 1.0);
    assert!((size.y - 180.0).abs() < 0.5 && size.x <= 560.5, "{size:?}");
    // a small dark logo is cropped too (L3)
    let dark = logo_info(&padded([5, 5, 5, 255], 120, 80, 160, 40));
    assert!(dark.dark);
    assert!((logo_fit(vec2(400.0, 200.0), dark.bounds, vec2(560.0, 180.0), 1.0) - vec2(320.0, 80.0)).length() < 0.01);
}

#[test]
fn dark_logo_halo_hugs_the_glyphs() {
    let bytes = padded([5, 5, 5, 255], 100, 80, 200, 40);
    let info = logo_info(&bytes);
    let boxsz = vec2(560.0, 180.0);
    let l = logo_layout(vec2(400.0, 200.0), &info, boxsz, 1.0);
    assert_eq!(l.uv, info.bounds, "drawn from the visible sub-rect");
    let halo = l.halo.expect("dark logo has a halo");
    assert!(l.rect.min.y > 0.0 && l.rect.max.y == boxsz.y, "anchored on the bottom of the box: {:?}", l.rect);
    let reach = (l.rect.min.x - halo.min.x)
        .max(l.rect.min.y - halo.min.y)
        .max(halo.max.x - l.rect.max.x)
        .max(halo.max.y - l.rect.max.y);
    assert!(reach <= 12.0 + 0.01 && reach > 0.0, "halo reaches ≤ 12 px past the glyph box: {reach}");
    // the halo texture is shaped like the glyphs: a hole in the logo has no halo in its middle only if far from glyphs;
    // here: the corner of the padded image far from the block is clear, the block's edge is lit, nothing is filled solid
    let img = logo_halo_image(&bytes, info.bounds).expect("halo image");
    let pad = LOGO_HALO_PAD as usize;
    assert_eq!(img.size, [200 + 2 * pad, 40 + 2 * pad]);
    assert_eq!(img.pixels[0].a(), 0, "far corner is clear");
    assert!(img.pixels[img.size[0] * (pad + 20) + pad - 1].a() > 0, "just outside the glyph edge is lit");
}

#[test]
fn halo_alpha_capped() {
    let bytes = padded([5, 5, 5, 255], 100, 80, 200, 40);
    let info = logo_info(&bytes);
    let img = logo_halo_image(&bytes, info.bounds).unwrap();
    let peak = img.pixels.iter().map(|p| p.a()).max().unwrap();
    assert!(peak <= LOGO_HALO_ALPHA && LOGO_HALO_ALPHA as f32 / 255.0 <= 0.25, "peak alpha ≤ 25 %: {peak}");
    assert!(peak > 0);
    assert!(img.pixels.iter().all(|p| p.r() == p.a() && p.g() == p.a() && p.b() == p.a()), "white, premultiplied");
}

#[test]
fn light_logo_has_no_halo() {
    let bytes = padded([240, 240, 240, 255], 100, 80, 200, 40);
    let info = logo_info(&bytes);
    assert!(!info.dark);
    let l = logo_layout(vec2(400.0, 200.0), &info, vec2(560.0, 180.0), 1.0);
    assert!(l.halo.is_none());
    assert_eq!(l.uv, info.bounds, "light logos are still cropped (L4)");
}

#[test]
fn upscale_cap_is_in_physical_pixels() {
    let info = logo_info(&padded([255, 255, 255, 255], 120, 80, 160, 40));
    let at = |ppp: f32| logo_fit(vec2(400.0, 200.0), info.bounds, vec2(560.0, 180.0), ppp);
    assert!((at(1.0) - vec2(320.0, 80.0)).length() < 0.01, "1x: 2 px per texture px");
    assert!((at(2.0) - vec2(160.0, 40.0)).length() < 0.01, "2x display: still 2 physical px per texture px, not 4");
    assert!((at(2.0).x * 2.0 / 160.0) <= LOGO_MAX_UPSCALE + 0.001);
}

fn known(dark: bool, ok: bool) -> LogoInfo {
    LogoInfo { dark, loaded: true, ok, ..LogoInfo::default() }
}

#[test]
fn nothing_is_drawn_until_the_final_texture_is_ready() {
    let unloaded = LogoInfo::default();
    assert_eq!(logo_choice::<u8>(None, 1.0, None, 0.0), None);
    // narrow pass not run yet → held
    assert_eq!(logo_choice(Some((1, unloaded)), 1.0, None, 0.0), Some((1, false)));
    // unpadded logo: drawn as soon as its pass ran
    assert_eq!(logo_choice(Some((1, known(false, true))), 1.0, None, 0.0), Some((1, true)));
    // padded logo (hint 4): narrow is known but the wide one is not there / not passed yet → still held, no small flash
    assert_eq!(logo_choice(Some((1, known(false, true))), 4.0, None, 0.5), Some((1, false)));
    assert_eq!(logo_choice(Some((1, known(false, true))), 4.0, Some((2, unloaded)), 0.5), Some((1, false)));
    // wide one passed → it is used
    assert_eq!(logo_choice(Some((1, known(false, true))), 4.0, Some((2, known(true, true))), 0.5), Some((2, true)));
}

#[test]
fn wide_fetch_failure_keeps_the_cropped_narrow_logo() {
    let narrow = Some((1, known(true, true)));
    // the wide pass ran but its bytes failed → narrow (cropped) is drawn, never the uncropped wide one
    assert_eq!(logo_choice(narrow, 4.0, Some((2, known(false, false))), 0.5), Some((1, true)));
    // the wide texture never arrives → after the wait the narrow one is drawn
    assert_eq!(logo_choice(narrow, 4.0, None, LOGO_WIDE_WAIT - 0.1), Some((1, false)));
    assert_eq!(logo_choice(narrow, 4.0, None, LOGO_WIDE_WAIT + 0.1), Some((1, true)));
}

/// C3-POLISH #6: while the logo is not ready its reserved box shows the text title, bottom-aligned like the logo; a
/// title long enough for two lines stays inside the box (never drawn up over the line above).
#[test]
fn pending_logo_box_shows_the_text_title_inside_the_box() {
    use egui::epaint::Shape;
    use jf_core::theme::Theme;
    let drawn = |name: &str| {
        let ctx = egui::Context::default();
        ctx.set_fonts(Theme::fonts());
        let t = Theme::placeholder();
        let mut boxr = Rect::NOTHING;
        let out = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx, |ui| {
                let top = ui.cursor().top();
                // a logo texture whose pass has not run: the pending state
                jf_core::ui::at_details::hero_title(ui, &t, name, Some(egui::TextureId::User(77)));
                boxr = Rect::from_min_size(pos2(ui.min_rect().left(), top), vec2(t.hero_logo_w, t.hero_logo_h));
            });
        });
        let text: Vec<(Rect, usize)> = out
            .shapes
            .iter()
            .filter_map(|c| match &c.shape {
                Shape::Text(s) => Some((s.visual_bounding_rect(), s.galley.rows.len())),
                _ => None,
            })
            .collect();
        assert_eq!(text.len(), 1, "one text title, not a blank box");
        (boxr, text[0].0, text[0].1)
    };
    let (b, r, rows) = drawn("Elephants Dream");
    assert_eq!(rows, 1);
    assert!(
        (r.bottom() - b.bottom()).abs() < 20.0 && r.top() >= b.top(),
        "one line sits at the box bottom: {r:?} in {b:?}"
    );
    let (b, r, rows) = drawn("Agent 327: Operation Barbershop and the Very Long Subtitle");
    assert_eq!(rows, 2, "two lines at most");
    assert!(
        r.top() >= b.top() - 0.5 && r.bottom() <= b.bottom() + 0.5,
        "two lines stay inside the box: {r:?} in {b:?}"
    );
}
