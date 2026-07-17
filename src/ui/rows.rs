//! Session list row widgets.

use eframe::egui::{self, Color32, RichText, Sense, Vec2};
use eframe::epaint::CornerRadius;

use super::theme::{self, short_ref};
use crate::provider::ProviderSession;
use crate::store::{ManualGroup, Priority};

pub(super) fn collapse_home(path: &str) -> String {
    if let Ok(home) = std::env::var("HOME") {
        if path.starts_with(&home) {
            return format!("~{}", &path[home.len()..]);
        }
    }
    path.to_string()
}

pub(super) enum RowAction {
    None,
    Focus,
    Assign(String),
    Unassign,
    SetPriority(Priority),
}

fn truncate(s: &str, max: usize) -> String {
    let t = s.replace('\n', " ");
    if t.chars().count() > max {
        let cut: String = t.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    } else {
        t
    }
}

fn accent_from_rgb(rgb: (u8, u8, u8), muted: bool) -> Color32 {
    if muted {
        theme::FG_FAINT
    } else {
        theme::readable_accent(Color32::from_rgb(rgb.0, rgb.1, rgb.2))
    }
}

/// Attention-tinted left bar + pills for provider / agent / attention.
pub(super) fn session_row(
    ui: &mut egui::Ui,
    s: &ProviderSession,
    selected: bool,
    in_manual: bool,
    priority: Priority,
    groups: &[ManualGroup],
) -> RowAction {
    let title = truncate(&s.title, 42);
    let cwd = s
        .cwd
        .as_deref()
        .map(collapse_home)
        .unwrap_or_else(|| "—".into());
    let cwd = truncate(&cwd, 40);
    let ref_id = short_ref(s);
    let muted = priority == Priority::Muted;
    let needs = s.attention == crate::attention::Attention::NeedsYou && !muted;

    let row_h = 48.0;
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), row_h), Sense::click());

    let fill = if selected && needs {
        theme::BG_NEEDS_SEL
    } else if selected {
        theme::BG_ROW_SELECTED
    } else if muted {
        theme::BG_MUTED
    } else if needs && resp.hovered() {
        theme::BG_NEEDS_HOVER
    } else if needs {
        theme::BG_NEEDS
    } else if priority == Priority::Important {
        theme::BG_IMPORTANT
    } else if s.is_focused {
        theme::BG_ROW_FOCUSED
    } else if resp.hovered() {
        theme::BG_ROW_HOVER
    } else if in_manual {
        Color32::from_rgb(36, 38, 54)
    } else {
        theme::BG_ROW
    };

    ui.painter().rect_filled(rect, CornerRadius::same(7), fill);

    let agent_color = accent_from_rgb(s.agent.rgb(), muted);
    let attn_color = accent_from_rgb(s.attention.rgb(), muted);
    let bar_color = if needs { attn_color } else { agent_color };
    let bar = egui::Rect::from_min_size(rect.min, Vec2::new(4.0, rect.height()));
    ui.painter()
        .rect_filled(bar, CornerRadius::same(2), bar_color);

    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(Vec2::new(12.0, 6.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );

    let focus_col = if s.is_focused {
        theme::GREEN
    } else {
        theme::FG_FAINT
    };
    child.label(
        RichText::new(if s.is_focused { "●" } else { "○" })
            .color(focus_col)
            .size(12.0),
    );
    child.add_space(6.0);

    if priority == Priority::Important {
        child.label(RichText::new("★").color(theme::AMBER).strong().size(13.0));
        child.add_space(4.0);
    }

    let pcol = if muted {
        theme::FG_FAINT
    } else {
        theme::provider_color(&s.provider)
    };
    theme::pill(&mut child, theme::provider_label(&s.provider), pcol);
    child.add_space(5.0);
    theme::pill(&mut child, s.agent.label(), agent_color);
    child.add_space(5.0);
    theme::pill(&mut child, s.attention.label(), attn_color);
    child.add_space(10.0);

    let title_color = if muted {
        theme::FG_FAINT
    } else {
        theme::FG_TITLE
    };
    child.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.label(RichText::new(&title).color(title_color).size(14.0));
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.label(RichText::new(&ref_id).size(12.0).color(pcol));
            ui.label(RichText::new("·").size(12.0).color(theme::FG_FAINT));
            ui.label(RichText::new(&cwd).size(12.0).color(theme::FG_META));
        });
    });

    ui.advance_cursor_after_rect(rect);
    ui.add_space(3.0);

    let mut action = RowAction::None;
    if resp.clicked() {
        action = RowAction::Focus;
    }
    resp.clone().on_hover_text(format!(
        "id        {}\nprovider  {}\nref       {}\ncwd       {}\nattention {}",
        s.id,
        s.provider,
        ref_id,
        s.cwd.as_deref().unwrap_or("—"),
        s.attention.label()
    ));
    resp.context_menu(|ui| {
        ui.label(
            RichText::new("Priority")
                .size(12.0)
                .strong()
                .color(theme::FG_META),
        );
        if ui.button("★ Important").clicked() {
            action = RowAction::SetPriority(Priority::Important);
            ui.close_menu();
        }
        if ui.button("Normal").clicked() {
            action = RowAction::SetPriority(Priority::Normal);
            ui.close_menu();
        }
        if ui.button("Muted").clicked() {
            action = RowAction::SetPriority(Priority::Muted);
            ui.close_menu();
        }
        ui.separator();
        ui.label(
            RichText::new("Assign to")
                .size(12.0)
                .strong()
                .color(theme::FG_META),
        );
        if groups.is_empty() {
            ui.label(
                RichText::new("(create a group in Tools)")
                    .size(12.0)
                    .color(theme::FG_META),
            );
        }
        for g in groups {
            if ui.button(&g.title).clicked() {
                action = RowAction::Assign(g.id.clone());
                ui.close_menu();
            }
        }
        ui.separator();
        if ui.button("Unassign").clicked() {
            action = RowAction::Unassign;
            ui.close_menu();
        }
        ui.separator();
        ui.label(
            RichText::new(format!("id  {}", s.id))
                .size(12.0)
                .color(theme::FG_META),
        );
    });
    action
}

/// Queue row — static needs-you emphasis, no flash.
pub(super) fn queue_row(
    ui: &mut egui::Ui,
    index: usize,
    s: &ProviderSession,
    selected: bool,
    priority: Priority,
) -> bool {
    let title = truncate(&s.title, 36);
    let ref_id = short_ref(s);
    let needs = s.attention == crate::attention::Attention::NeedsYou;

    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 40.0), Sense::click());

    let fill = if selected {
        theme::BG_NEEDS_SEL
    } else if resp.hovered() {
        theme::BG_NEEDS_HOVER
    } else if needs {
        theme::BG_NEEDS
    } else {
        theme::BG_QUEUE
    };
    ui.painter().rect_filled(rect, CornerRadius::same(7), fill);

    let attn = accent_from_rgb(s.attention.rgb(), false);
    ui.painter().rect_filled(
        egui::Rect::from_min_size(rect.min, Vec2::new(4.0, rect.height())),
        CornerRadius::same(2),
        attn,
    );

    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(Vec2::new(12.0, 6.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );

    let star = if priority == Priority::Important {
        "★ "
    } else {
        ""
    };
    child.label(
        RichText::new(format!("{star}{}", index + 1))
            .strong()
            .color(theme::AMBER)
            .size(13.0),
    );
    child.add_space(8.0);

    let pcol = theme::provider_color(&s.provider);
    theme::pill(&mut child, theme::provider_label(&s.provider), pcol);
    child.add_space(5.0);
    theme::pill(
        &mut child,
        s.agent.label(),
        accent_from_rgb(s.agent.rgb(), false),
    );
    child.add_space(5.0);
    theme::pill(&mut child, s.attention.label(), attn);
    child.add_space(10.0);
    child.label(RichText::new(&title).color(theme::FG_TITLE).size(14.0));
    child.add_space(8.0);
    child.label(RichText::new(&ref_id).size(12.0).color(pcol));

    ui.advance_cursor_after_rect(rect);
    ui.add_space(4.0);
    resp.clicked()
}
