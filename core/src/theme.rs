//! Every visual value in one place (R-THEME-1). Screens and parts never use literals. Written at 1080p; the
//! shell scales with `pixels_per_point` (R-THEME-3). The look is the Apple TV app (docs/DESIGN-appletv.md):
//! deep dark backgrounds, artwork does the colour, white pills, translucent bars, focus = lift + shadow.
//! Type is Inter (OFL, `core/assets/fonts/`), loaded by `apply`.
use egui::{Color32, FontFamily, FontId};

/// Inter weights embedded in the binary. Regular is egui's proportional family; the others are named families.
const FONTS: [(&str, &[u8]); 4] = [
    ("Inter-Regular", include_bytes!("../assets/fonts/Inter-Regular.ttf")),
    ("Inter-Medium", include_bytes!("../assets/fonts/Inter-Medium.ttf")),
    ("Inter-SemiBold", include_bytes!("../assets/fonts/Inter-SemiBold.ttf")),
    ("Inter-Bold", include_bytes!("../assets/fonts/Inter-Bold.ttf")),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Weight {
    Regular,
    Medium,
    SemiBold,
    Bold,
}

impl Weight {
    fn family(self) -> FontFamily {
        match self {
            Weight::Regular => FontFamily::Proportional,
            Weight::Medium => FontFamily::Name("Inter-Medium".into()),
            Weight::SemiBold => FontFamily::Name("Inter-SemiBold".into()),
            Weight::Bold => FontFamily::Name("Inter-Bold".into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    // colours
    pub bg: Color32,
    pub surface: Color32,
    pub surface_hi: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub accent: Color32,
    pub accent_text: Color32,
    pub error: Color32,
    pub favourite: Color32,
    pub progress: Color32,
    pub progress_track: Color32,
    pub scrim: Color32,
    pub skeleton: Color32,
    // focus
    pub focus_stroke: f32,
    pub focus_color: Color32,
    pub focus_gap: f32,
    // type (px at 1080p)
    pub title_size: f32,
    pub heading_size: f32,
    pub body_size: f32,
    pub small_size: f32,
    pub code_size: f32,
    // geometry
    pub margin: f32,
    pub gap: f32,
    pub row_gap: f32,
    pub radius: f32,
    pub poster_w: f32,
    pub poster_h: f32,
    pub thumb_w: f32,
    pub thumb_h: f32,
    /// P-LIB-14: banner-view strip (wide, ~1000×185 ratio).
    pub banner_w: f32,
    pub banner_h: f32,
    pub card_label_h: f32,
    pub button_h: f32,
    pub button_pad: f32,
    pub field_w: f32,
    pub topbar_h: f32,
    pub backdrop_h: f32,
    pub hero_h: f32,
    pub ep_thumb_w: f32,
    pub ep_thumb_h: f32,
    pub dropdown_w: f32,
    pub progress_h: f32,
    pub person_size: f32,
    // person page (F5)
    pub person_photo_w: f32,
    pub person_photo_h: f32,
    pub credit_poster_w: f32,
    pub credit_poster_h: f32,
    pub dim_scrim: Color32,
    // user settings (G12): profile picture on the Account tab
    pub avatar_size: f32,
    // widgets (W0): dropdowns, toggles, chips, tabs, dialogs, lists, forms
    pub menu_item_h: f32,
    pub menu_pad: f32,
    pub menu_max_h: f32,
    pub toggle_w: f32,
    pub toggle_h: f32,
    pub toggle_inset: f32,
    pub chip_h: f32,
    pub chip_pad: f32,
    pub tab_h: f32,
    pub tab_pad: f32,
    pub tab_underline: f32,
    pub dialog_w: f32,
    pub dialog_pad: f32,
    pub list_row_h: f32,
    pub form_label_w: f32,
    pub form_breakpoint: f32,
    // player OSD volume bar (G4, P-PLAY-19)
    pub volume_bar_w: f32,
    /// RR4 / P-PLAY-21: on-screen width of the seek-bar trickplay thumbnail.
    pub trickplay_w: f32,
    // library views (G11): A–Z bar, list rows, filters dialog
    pub az_w: f32,
    pub az_h: f32,
    pub list_thumb_h: f32,
    pub filters_w: f32,
    pub filters_h: f32,
    // section boxes (R-F2-1)
    pub section_border: Color32,
    pub section_border_w: f32,
    pub section_radius: f32,
    pub section_pad: f32,
    // My-order editing (R-F4-2)
    pub drop_marker_w: f32,
    pub ghost_scrim: Color32,
    // motion (seconds)
    pub scroll_time: f32,
    pub osd_hide_after: f32,
    pub search_debounce: f32,
    pub up_next_countdown: f32,
    /// G10: how far the skip-segment button lifts above the bottom margin while the OSD is showing.
    pub skip_lift: f32,
    // ---- NAV: desktop sidebar + toolbar ----
    pub sidebar_w: f32,
    /// Collapsed sidebar: icons only.
    pub sidebar_rail_w: f32,
    pub sidebar_pad: f32,
    pub sidebar_gap: f32,
    pub sidebar_item_h: f32,
    pub sidebar_user_h: f32,
    pub sidebar_avatar: f32,
    /// Where a row's label starts, from the row's left edge.
    pub sidebar_text_x: f32,
    pub sidebar_icon: f32,
    pub sidebar_icon_stroke: f32,
    pub sidebar_menu_w: f32,
    pub sidebar_fill: Color32,
    pub sidebar_hover: Color32,
    pub sidebar_selected: Color32,
    pub sidebar_ink: Color32,
    pub toolbar_h: f32,
    pub toolbar_button: f32,
    // ---- Apple TV look (D1) ----
    /// Translucent white over dark: resting secondary pills, chips, fields.
    pub glass: Color32,
    /// Brighter glass: the selected chip / segment when not focused.
    pub glass_hi: Color32,
    /// A focused control turns into a white plate with dark ink.
    pub focus_fill: Color32,
    pub focus_ink: Color32,
    /// Translucent "material" for the top pill bar, menus and dialogs (pre-darkened: no real blur).
    pub bar_fill: Color32,
    pub bar_stroke: Color32,
    pub menu_fill: Color32,
    pub toggle_on: Color32,
    /// Soft drop shadow under focused things.
    pub shadow_color: Color32,
    pub shadow_blur: f32,
    pub shadow_offset: f32,
    /// Light sheen across the top of a focused card.
    pub sheen: Color32,
    // radii
    pub card_radius: f32,
    pub field_radius: f32,
    pub menu_radius: f32,
    pub dialog_radius: f32,
    // focus motion
    /// Focused cards grow by this much (Apple TV ~1.08).
    pub focus_scale: f32,
    /// Focused buttons, chips and rows grow a little less.
    pub pill_focus_scale: f32,
    /// Seconds for focus in/out (ease-out cubic).
    pub focus_time: f32,
    // type
    pub hero_title_size: f32,
    // hero (Home + details)
    pub hero_full_h: f32,
    pub hero_fade_h: f32,
    pub hero_logo_w: f32,
    pub hero_logo_h: f32,
    pub hero_text_w: f32,
    // cards
    pub wide_w: f32,
    pub wide_h: f32,
    pub card_title_gap: f32,
    // pills and round icon buttons
    pub pill_h: f32,
    pub pill_pad: f32,
    pub round_button: f32,
    pub icon_stroke: f32,
    // top pill tab bar
    pub bar_h: f32,
    pub bar_inset: f32,
    pub bar_item_pad: f32,
    pub bar_top: f32,
    // badges (4K, HDR, CC ...)
    pub badge_h: f32,
    pub badge_pad: f32,
    pub badge_stroke: f32,
    // ---- Poster Flow (BRIEF-coverflow; measured in docs/coverflow-reference.md) ----
    /// Pure black stage under the covers and their reflections.
    pub flow_bg: Color32,
    /// Second label line under the centre cover.
    pub flow_sub: Color32,
    /// Label sizes as fractions of the view height (the iPod's 14 px on 320, scaled for a TV-distance screen).
    pub flow_label_frac: f32,
    pub flow_sub_frac: f32,
    /// RR8: seconds each screensaver backdrop stays up.
    pub slide_secs: f32,
    /// RR3: width of the play queue side panel.
    pub queue_w: f32,
}

impl Theme {
    /// The Apple TV look. (Kept under its old name: every caller builds the theme through this.)
    pub fn placeholder() -> Theme {
        let white = |a: u8| Color32::from_rgba_unmultiplied(0xff, 0xff, 0xff, a);
        Theme {
            bg: Color32::from_rgb(0x0a, 0x0a, 0x0c),
            surface: Color32::from_rgb(0x1c, 0x1c, 0x1e),
            surface_hi: Color32::from_rgb(0x2c, 0x2c, 0x2e),
            text: Color32::from_rgb(0xf5, 0xf5, 0xf7),
            muted: Color32::from_rgb(0x9a, 0x9a, 0xa0),
            accent: Color32::from_rgb(0xf5, 0xf5, 0xf7),
            accent_text: Color32::from_rgb(0x0a, 0x0a, 0x0c),
            error: Color32::from_rgb(0xff, 0x45, 0x3a),
            favourite: Color32::from_rgb(0xff, 0x37, 0x5f),
            progress: Color32::from_rgb(0xf5, 0xf5, 0xf7),
            progress_track: white(0x40),
            scrim: Color32::from_rgba_unmultiplied(0, 0, 0, 0xb8),
            skeleton: Color32::from_rgb(0x1c, 0x1c, 0x1e),
            focus_stroke: 3.0,
            focus_color: Color32::from_rgb(0xf5, 0xf5, 0xf7),
            focus_gap: 4.0,
            title_size: 40.0,
            heading_size: 24.0,
            body_size: 17.0,
            small_size: 14.0,
            code_size: 64.0,
            margin: 80.0,
            gap: 32.0,
            row_gap: 48.0,
            radius: 12.0,
            poster_w: 200.0,
            poster_h: 300.0,
            thumb_w: 384.0,
            thumb_h: 216.0,
            banner_w: 800.0,
            banner_h: 148.0,
            card_label_h: 52.0,
            button_h: 48.0,
            button_pad: 28.0,
            field_w: 520.0,
            topbar_h: 96.0,
            backdrop_h: 760.0,
            hero_h: 440.0,
            ep_thumb_w: 320.0,
            ep_thumb_h: 180.0,
            dropdown_w: 300.0,
            progress_h: 4.0,
            person_size: 150.0,
            person_photo_w: 200.0,
            person_photo_h: 300.0,
            credit_poster_w: 120.0,
            credit_poster_h: 180.0,
            dim_scrim: Color32::from_rgba_unmultiplied(0, 0, 0, 0x66),
            avatar_size: 120.0,
            menu_item_h: 48.0,
            menu_pad: 20.0,
            menu_max_h: 380.0,
            toggle_w: 56.0,
            toggle_h: 32.0,
            toggle_inset: 3.0,
            chip_h: 40.0,
            chip_pad: 20.0,
            tab_h: 48.0,
            tab_pad: 24.0,
            tab_underline: 3.0,
            dialog_w: 640.0,
            dialog_pad: 40.0,
            list_row_h: 60.0,
            form_label_w: 240.0,
            form_breakpoint: 900.0,
            volume_bar_w: 320.0,
            trickplay_w: 320.0,
            az_w: 36.0,
            az_h: 28.0,
            list_thumb_h: 72.0,
            filters_w: 1100.0,
            filters_h: 760.0,
            section_border: white(0x1a),
            section_border_w: 1.0,
            section_radius: 16.0,
            section_pad: 28.0,
            drop_marker_w: 4.0,
            ghost_scrim: Color32::from_rgba_unmultiplied(0x0a, 0x0a, 0x0c, 0x90),
            scroll_time: 0.2,
            osd_hide_after: 3.0,
            search_debounce: 0.3,
            up_next_countdown: 10.0,
            skip_lift: 200.0,
            sidebar_w: 260.0,
            sidebar_rail_w: 76.0,
            sidebar_pad: 14.0,
            sidebar_gap: 4.0,
            sidebar_item_h: 48.0,
            sidebar_user_h: 56.0,
            sidebar_avatar: 32.0,
            sidebar_text_x: 52.0,
            sidebar_icon: 20.0,
            sidebar_icon_stroke: 1.8,
            sidebar_menu_w: 232.0,
            sidebar_fill: Color32::from_rgb(0x16, 0x16, 0x18),
            sidebar_hover: Color32::from_rgba_unmultiplied(0xff, 0xff, 0xff, 0x14),
            sidebar_selected: Color32::from_rgba_unmultiplied(0xff, 0xff, 0xff, 0x26),
            sidebar_ink: Color32::from_rgb(0xc8, 0xc8, 0xcc),
            toolbar_h: 72.0,
            toolbar_button: 40.0,
            glass: white(0x1f),
            glass_hi: white(0x3d),
            focus_fill: Color32::from_rgb(0xf5, 0xf5, 0xf7),
            focus_ink: Color32::from_rgb(0x0a, 0x0a, 0x0c),
            bar_fill: Color32::from_rgba_unmultiplied(0x26, 0x26, 0x2a, 0xd0),
            bar_stroke: white(0x14),
            menu_fill: Color32::from_rgba_unmultiplied(0x2a, 0x2a, 0x2e, 0xf2),
            toggle_on: Color32::from_rgb(0x30, 0xd1, 0x58),
            shadow_color: Color32::from_rgba_unmultiplied(0, 0, 0, 0xa0),
            shadow_blur: 36.0,
            shadow_offset: 14.0,
            sheen: white(0x26),
            card_radius: 14.0,
            field_radius: 12.0,
            menu_radius: 20.0,
            dialog_radius: 28.0,
            focus_scale: 1.08,
            pill_focus_scale: 1.05,
            focus_time: 0.2,
            hero_title_size: 64.0,
            hero_full_h: 880.0,
            hero_fade_h: 420.0,
            hero_logo_w: 560.0,
            hero_logo_h: 180.0,
            hero_text_w: 720.0,
            wide_w: 400.0,
            wide_h: 225.0,
            card_title_gap: 12.0,
            pill_h: 56.0,
            pill_pad: 40.0,
            round_button: 56.0,
            icon_stroke: 2.5,
            bar_h: 56.0,
            bar_inset: 6.0,
            bar_item_pad: 24.0,
            bar_top: 24.0,
            badge_h: 24.0,
            badge_pad: 7.0,
            badge_stroke: 1.5,
            flow_bg: Color32::BLACK,
            flow_sub: Color32::from_rgb(0x9a, 0x9a, 0x9e),
            flow_label_frac: 0.042,
            flow_sub_frac: 0.032,
            slide_secs: 8.0,
            queue_w: 640.0,
        }
    }

    /// The type scale carries its weight, so every existing `font(size)` caller gets Apple's hierarchy:
    /// title sizes and up are bold, heading sizes semibold, everything smaller regular.
    pub fn font(&self, size: f32) -> FontId {
        let w = if size >= self.title_size {
            Weight::Bold
        } else if size >= self.heading_size {
            Weight::SemiBold
        } else {
            Weight::Regular
        };
        self.font_w(size, w)
    }

    /// A size in a chosen weight (labels on pills and the tab bar are medium/semibold at body size).
    pub fn font_w(&self, size: f32, w: Weight) -> FontId {
        FontId::new(size, w.family())
    }

    /// Eases a 0..1 focus amount (ease-out cubic): quick start, soft landing.
    pub fn ease(&self, x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        1.0 - (1.0 - x).powi(3)
    }

    /// egui drop shadow for focused things, `a` = focus amount 0..1.
    pub fn shadow(&self, a: f32) -> egui::epaint::Shadow {
        egui::epaint::Shadow {
            offset: [0, (self.shadow_offset * a) as i8],
            blur: (self.shadow_blur * a) as u8,
            spread: 0,
            color: self.shadow_color.gamma_multiply(a),
        }
    }

    /// Inter as the proportional family (regular) plus medium/semibold/bold named families; egui's own
    /// emoji/icon fonts stay behind each as fallbacks.
    pub fn fonts() -> egui::FontDefinitions {
        let mut defs = egui::FontDefinitions::default();
        let fallbacks: Vec<String> = defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
        for (name, bytes) in FONTS {
            defs.font_data.insert(name.to_string(), std::sync::Arc::new(egui::FontData::from_static(bytes)));
        }
        let mut regular = vec!["Inter-Regular".to_string()];
        regular.extend(fallbacks.iter().cloned());
        defs.families.insert(FontFamily::Proportional, regular);
        for w in [Weight::Medium, Weight::SemiBold, Weight::Bold] {
            let FontFamily::Name(name) = w.family() else { continue };
            let mut list = vec![name.to_string()];
            list.extend(fallbacks.iter().cloned());
            defs.families.insert(w.family(), list);
        }
        defs
    }

    /// Applies the theme to egui's own widgets (text fields, scroll bars) so they match.
    pub fn apply(&self, ctx: &egui::Context) {
        ctx.set_fonts(Self::fonts());
        let mut v = egui::Visuals::dark();
        v.panel_fill = self.bg;
        v.window_fill = self.surface;
        v.override_text_color = Some(self.text);
        v.selection.bg_fill = self.surface_hi;
        v.selection.stroke = egui::Stroke::new(self.focus_stroke, self.focus_color);
        v.widgets.inactive.bg_fill = self.surface;
        v.widgets.inactive.weak_bg_fill = self.surface;
        v.widgets.hovered.bg_fill = self.surface_hi;
        v.widgets.hovered.weak_bg_fill = self.surface_hi;
        v.widgets.active.bg_fill = self.surface_hi;
        v.text_cursor.stroke = egui::Stroke::new(2.0, self.text);
        // Text fields: rounded glass, no outline (focus is drawn by the screen).
        v.extreme_bg_color = self.surface_hi;
        let r = egui::CornerRadius::same(self.field_radius as u8);
        for w in [
            &mut v.widgets.noninteractive,
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
        ] {
            w.corner_radius = r;
            w.bg_stroke = egui::Stroke::NONE;
        }
        v.window_corner_radius = egui::CornerRadius::same(self.menu_radius as u8);
        v.window_stroke = egui::Stroke::new(1.0, self.bar_stroke);
        ctx.set_visuals(v);
        let mut s = (*ctx.style()).clone();
        s.text_styles.insert(egui::TextStyle::Body, self.font(self.body_size));
        s.text_styles.insert(egui::TextStyle::Button, self.font(self.body_size));
        s.text_styles.insert(egui::TextStyle::Heading, self.font(self.heading_size));
        s.text_styles.insert(egui::TextStyle::Small, self.font(self.small_size));
        s.spacing.item_spacing = egui::vec2(self.gap / 2.0, self.gap / 2.0);
        ctx.set_style(s);
    }
}
