//! Panel design tokens (Tokyo Night–adjacent) and small drawing helpers.

use eframe::egui::{self, Color32, FontId, RichText, Sense, Stroke, Vec2};
use eframe::epaint::CornerRadius;

// ── Palette ──────────────────────────────────────────────────────────────

pub const BG: Color32 = Color32::from_rgb(22, 23, 34);
pub const BG_ELEVATED: Color32 = Color32::from_rgb(26, 27, 38);
pub const BG_ROW: Color32 = Color32::from_rgb(30, 32, 48);
pub const BG_ROW_HOVER: Color32 = Color32::from_rgb(36, 40, 59);
pub const BG_ROW_SELECTED: Color32 = Color32::from_rgb(42, 52, 88);
pub const BG_ROW_FOCUSED: Color32 = Color32::from_rgb(34, 42, 64);
pub const BG_MUTED: Color32 = Color32::from_rgb(24, 25, 32);
pub const BG_IMPORTANT: Color32 = Color32::from_rgb(40, 36, 28);
pub const BG_QUEUE: Color32 = Color32::from_rgb(36, 28, 34);
pub const BG_QUEUE_HOVER: Color32 = Color32::from_rgb(48, 34, 42);
pub const BG_QUEUE_SEL: Color32 = Color32::from_rgb(58, 38, 48);
pub const BG_CHIP: Color32 = Color32::from_rgb(36, 40, 59);
pub const BG_INPUT: Color32 = Color32::from_rgb(28, 30, 44);

pub const FG: Color32 = Color32::from_rgb(192, 202, 245);
pub const FG_DIM: Color32 = Color32::from_rgb(86, 95, 137);
pub const FG_MUTED: Color32 = Color32::from_rgb(100, 105, 125);
pub const FG_SOFT: Color32 = Color32::from_rgb(169, 177, 214);

pub const BLUE: Color32 = Color32::from_rgb(122, 162, 247);
pub const GREEN: Color32 = Color32::from_rgb(158, 206, 106);
pub const AMBER: Color32 = Color32::from_rgb(224, 175, 104);
pub const PINK: Color32 = Color32::from_rgb(247, 118, 142);
pub const PURPLE: Color32 = Color32::from_rgb(187, 154, 247);

pub const PROVIDER_KITTY: Color32 = Color32::from_rgb(255, 158, 100);
pub const PROVIDER_TMUX: Color32 = Color32::from_rgb(115, 218, 202);
pub const PROVIDER_OTHER: Color32 = Color32::from_rgb(86, 95, 137);

pub(super) fn apply_theme(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = Vec2::new(8.0, 6.0);
    style.spacing.button_padding = Vec2::new(10.0, 5.0);
    style.spacing.indent = 12.0;
    style.visuals = egui::Visuals::dark();
    style.visuals.panel_fill = BG;
    style.visuals.window_fill = BG;
    style.visuals.extreme_bg_color = BG_INPUT;
    style.visuals.faint_bg_color = BG_ELEVATED;
    style.visuals.override_text_color = Some(FG_SOFT);
    style.visuals.widgets.noninteractive.fg_stroke.color = FG_SOFT;
    style.visuals.widgets.inactive.fg_stroke.color = FG_SOFT;
    style.visuals.widgets.inactive.bg_fill = BG_CHIP;
    style.visuals.widgets.hovered.bg_fill = BG_ROW_HOVER;
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(41, 46, 66);
    style.visuals.selection.bg_fill = Color32::from_rgb(61, 89, 161);
    style.visuals.widgets.noninteractive.bg_stroke = Stroke::NONE;
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(40, 44, 62));
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(55, 62, 88));
    ctx.set_style(style);
}

pub fn provider_color(provider: &str) -> Color32 {
    match provider {
        "kitty" => PROVIDER_KITTY,
        "tmux" => PROVIDER_TMUX,
        _ => PROVIDER_OTHER,
    }
}

pub fn provider_label(provider: &str) -> &'static str {
    match provider {
        "kitty" => "kitty",
        "tmux" => "tmux",
        "all" => "all",
        _ => "term",
    }
}

/// Short human ref for a session (not the full opaque id).
pub fn short_ref(s: &crate::provider::ProviderSession) -> String {
    match s.provider.as_str() {
        "tmux" => {
            if let Some(w) = s.focus_window_id {
                format!("@{w}")
            } else if let Some(ref k) = s.focus_key {
                k.split('|')
                    .next()
                    .and_then(|b| b.rsplit(':').next())
                    .unwrap_or("?")
                    .to_string()
            } else {
                s.id.rsplit(':').next().unwrap_or("?").to_string()
            }
        }
        "kitty" => {
            if let Some(t) = s.focus_tab_id {
                format!("t{t}")
            } else {
                s.id.rsplit(':').next().unwrap_or("?").to_string()
            }
        }
        _ => s.id.rsplit(':').next().unwrap_or(&s.id).to_string(),
    }
}

/// Compact filled pill: `label` on tinted background.
pub fn pill(ui: &mut egui::Ui, label: &str, fg: Color32, bg: Color32) {
    let pad = Vec2::new(7.0, 2.0);
    let font = FontId::proportional(11.0);
    let galley = ui.fonts(|f| f.layout_no_wrap(label.to_string(), font, fg));
    let size = galley.size() + pad * 2.0;
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(4), bg);
    let text_pos = egui::pos2(rect.min.x + pad.x, rect.center().y - galley.size().y / 2.0);
    ui.painter().galley(text_pos, galley, fg);
}

pub fn tinted_bg(c: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), alpha)
}

pub fn section_header(ui: &mut egui::Ui, icon: &str, title: &str, meta: &str, color: Color32) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(icon).color(color).size(13.0));
        ui.label(RichText::new(title).strong().size(13.0).color(FG));
        if !meta.is_empty() {
            ui.label(RichText::new(meta).small().color(FG_DIM));
        }
    });
}

pub fn stat_chip(ui: &mut egui::Ui, label: &str, value: &str, color: Color32) {
    egui::Frame::new()
        .fill(BG_CHIP)
        .corner_radius(CornerRadius::same(5))
        .inner_margin(egui::Margin::symmetric(8, 3))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.label(RichText::new(label).small().color(FG_DIM));
                ui.label(RichText::new(value).small().strong().color(color));
            });
        });
}

pub fn empty_state(ui: &mut egui::Ui, title: &str, hint: &str) {
    ui.add_space(28.0);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new(title).size(15.0).color(FG_SOFT));
        ui.add_space(6.0);
        ui.label(RichText::new(hint).small().color(FG_DIM));
    });
}
