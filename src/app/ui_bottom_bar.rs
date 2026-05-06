//! 底部工具栏：阵列/一览、长按文件名、版本号。

use super::DeckRefApp;
use crate::app_shell_layout::BOTTOM_BAR_HEIGHT;
use eframe::egui::{self, Rect};

pub(super) fn show_bottom_bar(app: &mut DeckRefApp, ctx: &egui::Context) {
    let screen = ctx.screen_rect();
    let bottom_rect = Rect::from_min_max(
        egui::Pos2::new(screen.min.x, screen.max.y - BOTTOM_BAR_HEIGHT),
        screen.max,
    );
    egui::Area::new(egui::Id::new("deckref_bottom_bar"))
        .order(egui::Order::Foreground)
        .fixed_pos(bottom_rect.min)
        .constrain(true)
        .show(ctx, |ui| {
            ui.set_min_size(egui::Vec2::new(screen.width(), BOTTOM_BAR_HEIGHT));
            let fill = ui.visuals().window_fill();
            let c = egui::Color32::from(fill);
            let a = (c.a() as f32 * 0.04).round() as u8;
            let bg = egui::Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a);
            ui.painter().rect_filled(
                Rect::from_min_size(egui::Pos2::ZERO, ui.available_size()),
                egui::Rounding::same(0.0),
                bg,
            );
            ui.with_layout(
                egui::Layout::left_to_right(egui::Align::Center).with_cross_justify(true),
                |ui| {
                    if ui
                        .selectable_label(app.grid_view, egui::RichText::new("阵列").size(13.0))
                        .on_hover_text("按网格、原比例排列，快速观看全貌；之后仍可自由拖动")
                        .clicked()
                    {
                        app.stack_layout.available_size = app.last_stack_size;
                        app.grid_arrange();
                    }
                    if ui
                        .selectable_label(!app.grid_view, egui::RichText::new("一览").size(13.0))
                        .on_hover_text("将图片打散并均匀分布，快速浏览文件夹内容")
                        .clicked()
                    {
                        app.stack_layout.available_size = app.last_stack_size;
                        app.scatter_spread();
                    }
                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            let gray = egui::Color32::from_rgba_unmultiplied(100, 100, 100, 160);
                            let version = env!("CARGO_PKG_VERSION");
                            ui.label(
                                egui::RichText::new(format!("v{}", version))
                                    .color(gray)
                                    .size(11.0),
                            );
                            let held_index = app
                                .long_press
                                .as_ref()
                                .map(|(i, _)| *i)
                                .or_else(|| app.picked_up.as_ref().map(|(i, _, _)| *i));
                            if let Some(idx) = held_index {
                                if let Some(card) = app.cards.get(idx) {
                                    let name = card
                                        .path
                                        .file_name()
                                        .and_then(|n| n.to_str())
                                        .unwrap_or(card.file_name.as_str());
                                    ui.add_space(8.0);
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(name)
                                                .color(gray)
                                                .size(11.0),
                                        )
                                        .truncate(),
                                    );
                                }
                            }
                        },
                    );
                },
            );
        });
}
