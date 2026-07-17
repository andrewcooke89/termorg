//! Panel theme.

use eframe::egui::{self, Color32, Vec2};

pub(super) fn apply_theme(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = Vec2::new(8.0, 6.0);
    style.spacing.button_padding = Vec2::new(10.0, 6.0);
    ctx.set_style(style);

    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = Color32::from_rgb(26, 27, 38);
    visuals.window_fill = Color32::from_rgb(26, 27, 38);
    visuals.override_text_color = Some(Color32::from_rgb(169, 177, 214));
    visuals.widgets.noninteractive.fg_stroke.color = Color32::from_rgb(169, 177, 214);
    visuals.widgets.inactive.fg_stroke.color = Color32::from_rgb(169, 177, 214);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(36, 40, 59);
    visuals.widgets.active.bg_fill = Color32::from_rgb(41, 46, 66);
    visuals.selection.bg_fill = Color32::from_rgb(61, 89, 161);
    ctx.set_visuals(visuals);
}
