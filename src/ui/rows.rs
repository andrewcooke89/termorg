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

/// Attention-tinted left bar + pills for provider / agent / attention.
pub(super) fn session_row(
    ui: &mut egui::Ui,
    s: &ProviderSession,
    selected: bool,
    in_manual: bool,
    priority: Priority,
    groups: &[ManualGroup],
    pulse: f32,
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

    let row_h = 46.0;
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), row_h), Sense::click());

    let mut fill = if selected {
        theme::BG_ROW_SELECTED
    } else if muted {
        theme::BG_MUTED
    } else if priority == Priority::Important {
        theme::BG_IMPORTANT
    } else if s.is_focused {
        theme::BG_ROW_FOCUSED
    } else if resp.hovered() {
        theme::BG_ROW_HOVER
    } else if in_manual {
        Color32::from_rgb(32, 34, 46)
    } else {
        theme::BG_ROW
    };

    // Subtle pulse when needs you
    if needs {
        let boost = (pulse * 14.0) as u8;
        fill = Color32::from_rgb(
            fill.r().saturating_add(boost / 2),
            fill.g().saturating_add(boost / 4),
            fill.b().saturating_add(boost / 3),
        );
    }

    ui.painter().rect_filled(rect, CornerRadius::same(7), fill);

    // Left attention / agent accent bar
    let (ar, ag, ab) = s.agent.rgb();
    let agent_color = if muted {
        theme::FG_DIM
    } else {
        Color32::from_rgb(ar, ag, ab)
    };
    let (tr, tg, tb) = s.attention.rgb();
    let attn_color = if muted {
        theme::FG_DIM
    } else {
        Color32::from_rgb(tr, tg, tb)
    };
    let bar_color = if needs { attn_color } else { agent_color };
    let bar = egui::Rect::from_min_size(rect.min, Vec2::new(3.5, rect.height()));
    ui.painter()
        .rect_filled(bar, CornerRadius::same(2), bar_color);

    // Content
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(Vec2::new(12.0, 5.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );

    // Focus dot
    let focus_col = if s.is_focused {
        theme::GREEN
    } else {
        theme::FG_DIM
    };
    child.label(
        RichText::new(if s.is_focused { "●" } else { "○" })
            .color(focus_col)
            .size(11.0),
    );
    child.add_space(6.0);

    if priority == Priority::Important {
        child.label(RichText::new("★").color(theme::AMBER).strong());
        child.add_space(4.0);
    }

    // Provider pill
    let pcol = theme::provider_color(&s.provider);
    let plabel = theme::provider_label(&s.provider);
    let pbg = if muted {
        theme::tinted_bg(theme::FG_DIM, 40)
    } else {
        theme::tinted_bg(pcol, 48)
    };
    theme::pill(
        &mut child,
        plabel,
        if muted { theme::FG_MUTED } else { pcol },
        pbg,
    );
    child.add_space(4.0);

    // Agent pill
    theme::pill(
        &mut child,
        s.agent.label(),
        if muted { theme::FG_MUTED } else { agent_color },
        if muted {
            theme::tinted_bg(theme::FG_DIM, 36)
        } else {
            theme::tinted_bg(agent_color, 42)
        },
    );
    child.add_space(4.0);

    // Attention pill
    theme::pill(
        &mut child,
        s.attention.label(),
        if muted { theme::FG_MUTED } else { attn_color },
        if muted {
            theme::tinted_bg(theme::FG_DIM, 36)
        } else {
            theme::tinted_bg(attn_color, 48)
        },
    );
    child.add_space(8.0);

    // Title + meta
    let title_color = if muted { theme::FG_MUTED } else { theme::FG };
    child.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        ui.label(RichText::new(&title).color(title_color).size(13.0));
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.label(RichText::new(&ref_id).small().color(pcol));
            ui.label(RichText::new("·").small().color(theme::FG_DIM));
            ui.label(RichText::new(&cwd).small().color(theme::FG_DIM));
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
                .small()
                .strong()
                .color(theme::FG_DIM),
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
                .small()
                .strong()
                .color(theme::FG_DIM),
        );
        if groups.is_empty() {
            ui.label(
                RichText::new("(create a group in Tools)")
                    .small()
                    .color(theme::FG_DIM),
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
                .small()
                .color(theme::FG_DIM),
        );
    });
    action
}

/// Hero queue row — larger, attention-forward.
pub(super) fn queue_row(
    ui: &mut egui::Ui,
    index: usize,
    s: &ProviderSession,
    selected: bool,
    priority: Priority,
    pulse: f32,
) -> bool {
    let title = truncate(&s.title, 36);
    let ref_id = short_ref(s);
    let needs = s.attention == crate::attention::Attention::NeedsYou;

    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 36.0), Sense::click());

    let mut fill = if selected {
        theme::BG_QUEUE_SEL
    } else if resp.hovered() {
        theme::BG_QUEUE_HOVER
    } else {
        theme::BG_QUEUE
    };
    if needs {
        let boost = (pulse * 18.0) as u8;
        fill = Color32::from_rgb(
            fill.r().saturating_add(boost),
            fill.g().saturating_add(boost / 3),
            fill.b().saturating_add(boost / 2),
        );
    }
    ui.painter().rect_filled(rect, CornerRadius::same(6), fill);

    // left bar
    let (tr, tg, tb) = s.attention.rgb();
    let attn = Color32::from_rgb(tr, tg, tb);
    ui.painter().rect_filled(
        egui::Rect::from_min_size(rect.min, Vec2::new(3.5, rect.height())),
        CornerRadius::same(2),
        attn,
    );

    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(Vec2::new(10.0, 5.0)))
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
            .size(12.0),
    );
    child.add_space(8.0);

    let pcol = theme::provider_color(&s.provider);
    theme::pill(
        &mut child,
        theme::provider_label(&s.provider),
        pcol,
        theme::tinted_bg(pcol, 50),
    );
    child.add_space(4.0);

    let (ar, ag, ab) = s.agent.rgb();
    let agent_c = Color32::from_rgb(ar, ag, ab);
    theme::pill(
        &mut child,
        s.agent.label(),
        agent_c,
        theme::tinted_bg(agent_c, 45),
    );
    child.add_space(4.0);
    theme::pill(
        &mut child,
        s.attention.label(),
        attn,
        theme::tinted_bg(attn, 55),
    );
    child.add_space(8.0);
    child.label(RichText::new(&title).color(theme::FG).size(13.0));
    child.add_space(6.0);
    child.label(RichText::new(&ref_id).small().color(pcol));

    ui.advance_cursor_after_rect(rect);
    ui.add_space(3.0);
    resp.clicked()
}
