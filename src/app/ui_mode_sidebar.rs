//! 左侧模式切换条（叠加层）。

use super::DeckRefApp;
use crate::app_shell_layout::{
    BOTTOM_BAR_HEIGHT, MODE_SIDEBAR_WIDTH, TOP_BAR_HEIGHT,
};
use crate::mode::AppMode;
use eframe::egui::{self, Rect};

pub(super) fn show_mode_sidebar(app: &mut DeckRefApp, ctx: &egui::Context) {
    let screen = ctx.screen_rect();
    egui::Area::new(egui::Id::new("deckref_mode_sidebar"))
        .order(egui::Order::Foreground)
        .fixed_pos(egui::Pos2::new(screen.min.x, screen.min.y + TOP_BAR_HEIGHT))
        .constrain(true)
        .show(ctx, |ui| {
            ui.set_min_size(egui::Vec2::new(
                MODE_SIDEBAR_WIDTH,
                screen.height() - TOP_BAR_HEIGHT - BOTTOM_BAR_HEIGHT,
            ));
            let fill = ui.visuals().window_fill();
            let c = egui::Color32::from(fill);
            let a = (c.a() as f32 * 0.04).round() as u8;
            let bg = egui::Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a);
            ui.painter().rect_filled(
                Rect::from_min_size(egui::Pos2::ZERO, ui.available_size()),
                egui::Rounding::same(0.0),
                bg,
            );
            let button_block_height = 36.0 * 2.0 + 6.0;
            let top_space = (ui.available_height() - button_block_height) / 2.0;
            ui.add_space(top_space.max(0.0));
            let size = 36.0;
            let rounding = 8.0;
            for &m in AppMode::all() {
                let selected = app.mode == m;
                let btn = egui::Button::new(egui::RichText::new(m.label()).size(12.0))
                    .min_size(egui::Vec2::splat(size))
                    .rounding(egui::Rounding::same(rounding));
                let mut resp = ui.add(btn);
                if selected {
                    resp = resp.highlight();
                }
                if resp.clicked() {
                    app.switch_mode(m);
                }
                resp.on_hover_text(match m {
                    AppMode::Default => "默认模式：监听主文件夹中的图片",
                    AppMode::Inspiration => "灵感模式：展示最近使用的图片与 Ref/Screenshots 截图；按下 Shift 后约 5 秒内截屏会自动保存并显示",
                    AppMode::Board => "画板模式：画布布局持久化到当前目录的 .deckref_layout.json；切换目录或重启后恢复",
                });
                ui.add_space(6.0);
            }
        });
}
