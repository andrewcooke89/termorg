//! Session list row widgets.

use eframe::egui::{self, Color32, RichText, Sense, Vec2};
use eframe::epaint::CornerRadius;

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

pub(super) fn session_row(
    ui: &mut egui::Ui,
    s: &ProviderSession,
    selected: bool,
    in_manual: bool,
    priority: Priority,
    groups: &[ManualGroup],
) -> RowAction {
    let focus = if s.is_focused { "●" } else { "○" };
    let title = s.title.replace('\n', " ");
    let title = if title.len() > 48 {
        format!("{}…", &title[..45])
    } else {
        title
    };
    let cwd = s
        .cwd
        .as_deref()
        .map(collapse_home)
        .unwrap_or_else(|| "—".into());

    let muted = priority == Priority::Muted;
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 40.0), Sense::click());
    let fill = if selected {
        Color32::from_rgb(45, 55, 90)
    } else if muted {
        Color32::from_rgb(24, 25, 32)
    } else if priority == Priority::Important {
        Color32::from_rgb(40, 38, 28)
    } else if s.is_focused {
        Color32::from_rgb(36, 40, 59)
    } else if resp.hovered() {
        Color32::from_rgb(34, 38, 56)
    } else if in_manual {
        Color32::from_rgb(32, 34, 42)
    } else {
        Color32::from_rgb(30, 32, 48)
    };
    ui.painter().rect_filled(rect, CornerRadius::same(6), fill);

    let (ar, ag, ab) = s.agent.rgb();
    let agent_color = if muted {
        Color32::from_rgb(70, 75, 95)
    } else {
        Color32::from_rgb(ar, ag, ab)
    };
    let bar = egui::Rect::from_min_size(rect.min, Vec2::new(4.0, rect.height()));
    ui.painter()
        .rect_filled(bar, CornerRadius::same(2), agent_color);

    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(Vec2::new(12.0, 5.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child.label(RichText::new(format!("{focus}  ")).color(if s.is_focused {
        Color32::from_rgb(158, 206, 106)
    } else {
        Color32::from_rgb(86, 95, 137)
    }));
    if priority == Priority::Important {
        child.colored_label(
            Color32::from_rgb(224, 175, 104),
            RichText::new(" ★ ").strong(),
        );
    } else if muted {
        child.colored_label(Color32::from_rgb(86, 95, 137), RichText::new(" · ").small());
    }
    child.colored_label(
        agent_color,
        RichText::new(format!(" {} ", s.agent.label()))
            .strong()
            .small(),
    );
    let (tr, tg, tb) = s.attention.rgb();
    let attn_color = if muted {
        Color32::from_rgb(70, 75, 95)
    } else {
        Color32::from_rgb(tr, tg, tb)
    };
    child.add_space(4.0);
    child.colored_label(
        attn_color,
        RichText::new(format!(" {} ", s.attention.label()))
            .strong()
            .small(),
    );
    child.add_space(6.0);
    let title_color = if muted {
        Color32::from_rgb(100, 105, 125)
    } else {
        Color32::from_rgb(192, 202, 245)
    };
    child.vertical(|ui| {
        ui.label(RichText::new(&title).color(title_color));
        ui.label(
            RichText::new(format!("{}  ·  {}", s.id, cwd))
                .small()
                .color(Color32::from_rgb(86, 95, 137)),
        );
    });
    ui.advance_cursor_after_rect(rect);
    ui.add_space(4.0);

    let mut action = RowAction::None;
    if resp.clicked() {
        action = RowAction::Focus;
    }
    resp.context_menu(|ui| {
        ui.label(RichText::new("Priority").small().strong());
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
        ui.label(RichText::new("Assign to").small().strong());
        if groups.is_empty() {
            ui.label(
                RichText::new("(create a group above)")
                    .small()
                    .color(Color32::from_rgb(86, 95, 137)),
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
    });
    action
}
