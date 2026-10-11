//! Every visual value in one place (R-THEME-1). Screens and parts never use literals. Written at 1080p; the
//! shell scales with `pixels_per_point` (R-THEME-3). `placeholder()` is the neutral dark look until the real
//! design arrives (BRIEF-design).
use egui::Color32;

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
    /// RR3: width of the play queue side panel.
    pub queue_w: f32,
}

impl Theme {
    pub fn placeholder() -> Theme {
        Theme {
            bg: Color32::from_rgb(0x10, 0x10, 0x10),
            surface: Color32::from_rgb(0x1c, 0x1c, 0x1c),
            surface_hi: Color32::from_rgb(0x2a, 0x2a, 0x2a),
            text: Color32::from_rgb(0xec, 0xec, 0xec),
            muted: Color32::from_rgb(0x9a, 0x9a, 0x9a),
            accent: Color32::from_rgb(0xe0, 0xe0, 0xe0),
            accent_text: Color32::from_rgb(0x10, 0x10, 0x10),
            error: Color32::from_rgb(0xf0, 0x6a, 0x6a),
            favourite: Color32::from_rgb(0xf0, 0x5a, 0x8a),
            progress: Color32::from_rgb(0xe0, 0xe0, 0xe0),
            progress_track: Color32::from_rgba_unmultiplied(0xff, 0xff, 0xff, 0x40),
            scrim: Color32::from_rgba_unmultiplied(0, 0, 0, 0xb0),
            skeleton: Color32::from_rgb(0x22, 0x22, 0x22),
            focus_stroke: 3.0,
            focus_color: Color32::WHITE,
            focus_gap: 3.0,
            title_size: 34.0,
            heading_size: 22.0,
            body_size: 16.0,
            small_size: 13.0,
            code_size: 64.0,
            margin: 64.0,
            gap: 24.0,
            row_gap: 40.0,
            radius: 6.0,
            poster_w: 180.0,
            poster_h: 270.0,
            thumb_w: 320.0,
            thumb_h: 180.0,
            banner_w: 800.0,
            banner_h: 148.0,
            card_label_h: 44.0,
            button_h: 44.0,
            button_pad: 20.0,
            field_w: 520.0,
            topbar_h: 72.0,
            backdrop_h: 600.0,
            hero_h: 400.0,
            ep_thumb_w: 224.0,
            ep_thumb_h: 126.0,
            dropdown_w: 280.0,
            progress_h: 4.0,
            person_size: 150.0,
            person_photo_w: 200.0,
            person_photo_h: 300.0,
            credit_poster_w: 112.0,
            credit_poster_h: 168.0,
            dim_scrim: Color32::from_rgba_unmultiplied(0, 0, 0, 0x66),
            avatar_size: 120.0,
            menu_item_h: 44.0,
            menu_pad: 16.0,
            menu_max_h: 360.0,
            toggle_w: 56.0,
            toggle_h: 30.0,
            toggle_inset: 4.0,
            chip_h: 40.0,
            chip_pad: 16.0,
            tab_h: 44.0,
            tab_pad: 20.0,
            tab_underline: 3.0,
            dialog_w: 640.0,
            dialog_pad: 32.0,
            list_row_h: 56.0,
            form_label_w: 240.0,
            form_breakpoint: 900.0,
            volume_bar_w: 320.0,
            trickplay_w: 320.0,
            az_w: 36.0,
            az_h: 28.0,
            list_thumb_h: 72.0,
            filters_w: 1100.0,
            filters_h: 760.0,
            section_border: Color32::from_rgb(0x3a, 0x3a, 0x3a),
            section_border_w: 1.0,
            section_radius: 6.0,
            section_pad: 24.0,
            drop_marker_w: 4.0,
            ghost_scrim: Color32::from_rgba_unmultiplied(0x10, 0x10, 0x10, 0x90),
            scroll_time: 0.15,
            osd_hide_after: 3.0,
            search_debounce: 0.3,
            up_next_countdown: 10.0,
            skip_lift: 200.0,
            queue_w: 640.0,
        }
    }

    pub fn font(&self, size: f32) -> egui::FontId {
        egui::FontId::proportional(size)
    }

    /// Applies the theme to egui's own widgets (text fields, scroll bars) so they match.
    pub fn apply(&self, ctx: &egui::Context) {
        let mut v = egui::Visuals::dark();
        v.panel_fill = self.bg;
        v.window_fill = self.surface;
        v.extreme_bg_color = self.surface;
        v.override_text_color = Some(self.text);
        v.selection.bg_fill = self.surface_hi;
        v.selection.stroke = egui::Stroke::new(self.focus_stroke, self.focus_color);
        v.widgets.inactive.bg_fill = self.surface;
        v.widgets.inactive.weak_bg_fill = self.surface;
        v.widgets.hovered.bg_fill = self.surface_hi;
        v.widgets.hovered.weak_bg_fill = self.surface_hi;
        v.widgets.active.bg_fill = self.surface_hi;
        v.text_cursor.stroke = egui::Stroke::new(2.0, self.text);
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
