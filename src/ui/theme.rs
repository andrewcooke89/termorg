//! Panel design system — contrast-first dark UI.
//!
//! Hierarchy (bright → dim):
//!   FG_TITLE  primary titles / values
//!   FG_BODY   default reading text
//!   FG_META   secondary (cwd, chip labels, shortcuts)
//!   FG_FAINT  decorative only (inactive dots, separators)
//!
//! Accents are saturated but always drawn on a solid surface so they stay
//! readable; pills use a mixed background + lightened label, never pure
//! low-alpha tints alone.

use eframe::egui::{self, Color32, FontId, RichText, Sense, Stroke, Vec2};
use eframe::epaint::CornerRadius;

// ── Surfaces ─────────────────────────────────────────────────────────────
// Slightly lifted greys so body text has more room to breathe.

pub const BG: Color32 = Color32::from_rgb(24, 25, 36);
pub const BG_ELEVATED: Color32 = Color32::from_rgb(30, 32, 46);
pub const BG_ROW: Color32 = Color32::from_rgb(34, 36, 52);
pub const BG_ROW_HOVER: Color32 = Color32::from_rgb(42, 46, 66);
pub const BG_ROW_SELECTED: Color32 = Color32::from_rgb(48, 58, 92);
pub const BG_ROW_FOCUSED: Color32 = Color32::from_rgb(40, 48, 72);
pub const BG_MUTED: Color32 = Color32::from_rgb(28, 29, 38);
pub const BG_IMPORTANT: Color32 = Color32::from_rgb(48, 42, 32);
/// Static needs-you surface (no animation).
pub const BG_NEEDS: Color32 = Color32::from_rgb(52, 34, 42);
pub const BG_NEEDS_HOVER: Color32 = Color32::from_rgb(62, 40, 50);
pub const BG_NEEDS_SEL: Color32 = Color32::from_rgb(72, 44, 56);
pub const BG_QUEUE: Color32 = Color32::from_rgb(42, 32, 40);
pub const BG_CHIP: Color32 = Color32::from_rgb(40, 44, 62);
pub const BG_INPUT: Color32 = Color32::from_rgb(32, 34, 50);
pub const BG_PILL: Color32 = Color32::from_rgb(38, 42, 60);

// ── Text ─────────────────────────────────────────────────────────────────

/// Titles, primary values — highest contrast.
pub const FG_TITLE: Color32 = Color32::from_rgb(230, 234, 252);
/// Default body / session titles.
pub const FG: Color32 = Color32::from_rgb(214, 220, 245);
/// Soft body (status, secondary labels).
pub const FG_SOFT: Color32 = Color32::from_rgb(186, 194, 230);
/// Meta: cwd, chip labels, shortcuts — must stay readable on BG_ROW.
pub const FG_META: Color32 = Color32::from_rgb(148, 158, 198);
/// Faint decorative only.
pub const FG_FAINT: Color32 = Color32::from_rgb(110, 118, 155);
/// Legacy alias used across the UI.
pub const FG_DIM: Color32 = FG_META;

// ── Accents (slightly lifted for on-dark readability) ────────────────────

pub const BLUE: Color32 = Color32::from_rgb(130, 170, 255);
pub const GREEN: Color32 = Color32::from_rgb(166, 214, 118);
pub const AMBER: Color32 = Color32::from_rgb(232, 188, 120);
pub const PINK: Color32 = Color32::from_rgb(255, 130, 155);
pub const PURPLE: Color32 = Color32::from_rgb(198, 168, 255);

pub const PROVIDER_KITTY: Color32 = Color32::from_rgb(255, 170, 120);
pub const PROVIDER_TMUX: Color32 = Color32::from_rgb(120, 220, 200);
pub const PROVIDER_OTHER: Color32 = Color32::from_rgb(148, 158, 198);

pub const WHITE: Color32 = Color32::from_rgb(255, 255, 255);

pub(super) fn apply_theme(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = Vec2::new(8.0, 6.0);
    style.spacing.button_padding = Vec2::new(10.0, 5.0);
    style.spacing.indent = 12.0;
    // Prefer a slightly larger default text scale for readability.
    style
        .text_styles
        .insert(egui::TextStyle::Body, FontId::proportional(14.0));
    style
        .text_styles
        .insert(egui::TextStyle::Button, FontId::proportional(13.0));
    style
        .text_styles
        .insert(egui::TextStyle::Small, FontId::proportional(12.0));
    style
        .text_styles
        .insert(egui::TextStyle::Heading, FontId::proportional(18.0));

    style.visuals = egui::Visuals::dark();
    style.visuals.panel_fill = BG;
    style.visuals.window_fill = BG;
    style.visuals.extreme_bg_color = BG_INPUT;
    style.visuals.faint_bg_color = BG_ELEVATED;
    style.visuals.override_text_color = Some(FG);
    style.visuals.widgets.noninteractive.fg_stroke.color = FG_SOFT;
    style.visuals.widgets.inactive.fg_stroke.color = FG;
    style.visuals.widgets.inactive.bg_fill = BG_CHIP;
    style.visuals.widgets.hovered.bg_fill = BG_ROW_HOVER;
    style.visuals.widgets.hovered.fg_stroke.color = FG_TITLE;
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(50, 56, 80);
    style.visuals.selection.bg_fill = Color32::from_rgb(70, 100, 175);
    style.visuals.widgets.noninteractive.bg_stroke = Stroke::NONE;
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(52, 58, 82));
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(70, 80, 115));
    ctx.set_style(style);
}

pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    Color32::from_rgb(
        (a.r() as f32 + (b.r() as f32 - a.r() as f32) * t).round() as u8,
        (a.g() as f32 + (b.g() as f32 - a.g() as f32) * t).round() as u8,
        (a.b() as f32 + (b.b() as f32 - a.b() as f32) * t).round() as u8,
    )
}

/// Lift accent so it stays legible as pill text on dark chips.
pub fn readable_accent(c: Color32) -> Color32 {
    let luma = 0.299 * c.r() as f32 + 0.587 * c.g() as f32 + 0.114 * c.b() as f32;
    if luma < 150.0 {
        mix(c, WHITE, 0.28)
    } else {
        c
    }
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

/// High-contrast pill: solid mixed chip + lightened accent label.
pub fn pill(ui: &mut egui::Ui, label: &str, accent: Color32) {
    let fg = readable_accent(accent);
    let bg = mix(BG_PILL, accent, 0.22);
    let pad = Vec2::new(8.0, 3.0);
    let font = FontId::proportional(12.0);
    let galley = ui.fonts(|f| f.layout_no_wrap(label.to_string(), font, fg));
    let size = galley.size() + pad * 2.0;
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(5), bg);
    // subtle edge so pills separate from row fill
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(5),
        Stroke::new(1.0, mix(bg, fg, 0.18)),
        egui::StrokeKind::Inside,
    );
    let text_pos = egui::pos2(rect.min.x + pad.x, rect.center().y - galley.size().y / 2.0);
    ui.painter().galley(text_pos, galley, fg);
}

pub fn section_header(ui: &mut egui::Ui, icon: &str, title: &str, meta: &str, color: Color32) {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let mark = match icon {
            "◆" => "+",
            "▶" => ">",
            "◎" => "!",
            "☰" => "=",
            "✦" => "*",
            other => other,
        };
        ui.label(
            RichText::new(mark)
                .color(readable_accent(color))
                .size(14.0)
                .strong(),
        );
        ui.label(RichText::new(title).strong().size(14.0).color(FG_TITLE));
        if !meta.is_empty() {
            ui.label(RichText::new(meta).size(12.0).color(FG_META));
        }
    });
}

pub fn stat_chip(ui: &mut egui::Ui, label: &str, value: &str, color: Color32) {
    egui::Frame::new()
        .fill(BG_CHIP)
        .corner_radius(CornerRadius::same(6))
        .inner_margin(egui::Margin::symmetric(9, 4))
        .stroke(Stroke::new(1.0, Color32::from_rgb(52, 58, 82)))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 5.0;
                ui.label(RichText::new(label).size(12.0).color(FG_META));
                ui.label(
                    RichText::new(value)
                        .size(12.0)
                        .strong()
                        .color(readable_accent(color)),
                );
            });
        });
}

pub fn empty_state(ui: &mut egui::Ui, title: &str, hint: &str) {
    ui.add_space(28.0);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new(title).size(16.0).color(FG_SOFT));
        ui.add_space(6.0);
        ui.label(RichText::new(hint).size(13.0).color(FG_META));
    });
}
