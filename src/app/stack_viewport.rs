//! 中央堆叠区：视口补偿、中键/空格平移、滚轮缩放及世界↔屏幕变换。

use super::DeckRefApp;
use crate::board_view;
use crate::mode::AppMode;
use eframe::egui::{self, Pos2, Vec2};

/// 画板 / 默认·灵感 共用的世界坐标 ↔ 屏幕坐标（本帧快照，在 pan/zoom 之后构造）。
pub(super) struct ViewTransform {
    is_board: bool,
    screen_center_v: Vec2,
    view_offset: Vec2,
    view_scale: f32,
}

impl ViewTransform {
    #[inline]
    pub(super) fn view_scale(&self) -> f32 {
        self.view_scale
    }

    pub(super) fn snapshot(app: &DeckRefApp, screen_center: Pos2, is_board: bool) -> Self {
        let (view_offset, view_scale) = if is_board {
            (app.board_view_offset, app.board_view_scale)
        } else {
            (app.view_offset, app.view_scale)
        };
        Self {
            is_board,
            screen_center_v: screen_center.to_vec2(),
            view_offset,
            view_scale,
        }
    }

    #[inline]
    pub(super) fn world_to_screen(&self, w: Pos2) -> Pos2 {
        let p = if self.is_board {
            board_view::canvas_to_screen(w.to_vec2(), self.screen_center_v, self.view_offset, self.view_scale)
        } else {
            let v = w.to_vec2() - self.view_offset;
            Vec2::new(
                self.screen_center_v.x + v.x * self.view_scale,
                self.screen_center_v.y + v.y * self.view_scale,
            )
        };
        Pos2::new(p.x, p.y)
    }

    #[inline]
    pub(super) fn screen_to_world(&self, s: Pos2) -> Pos2 {
        let p = if self.is_board {
            board_view::screen_to_canvas(s.to_vec2(), self.screen_center_v, self.view_offset, self.view_scale)
        } else {
            let v = Vec2::new(s.x - self.screen_center_v.x, s.y - self.screen_center_v.y) / self.view_scale;
            self.view_offset + v
        };
        Pos2::new(p.x, p.y)
    }
}

/// 画板模式：内容区中心在物理屏幕上的位置（用于 resize 补偿）；非画板时退回内容区中心。
pub(super) fn board_screen_center_for_compensation(ui: &egui::Ui, screen_center: Pos2, is_board: bool) -> Vec2 {
    if is_board {
        ui.ctx()
            .input(|i| {
                i.viewport()
                    .outer_rect
                    .map(|outer| outer.min.to_vec2() + screen_center.to_vec2())
            })
            .unwrap_or(screen_center.to_vec2())
    } else {
        screen_center.to_vec2()
    }
}

/// 窗口 resize 补偿 + 中键/空格平移。
pub(super) fn apply_resize_and_pan(
    app: &mut DeckRefApp,
    ui: &mut egui::Ui,
    screen_center: Pos2,
    board_screen_center_for_compensation: Vec2,
) {
    if app.mode == AppMode::Board {
        board_view::apply_resize_compensation(
            &mut app.board_view_offset,
            app.board_view_scale,
            board_screen_center_for_compensation,
            app.last_board_screen_center,
        );
    } else {
        let c = screen_center.to_vec2();
        board_view::apply_resize_compensation(
            &mut app.view_offset,
            app.view_scale,
            c,
            app.last_stack_screen_center,
        );
    }

    let middle_down = ui.input(|i| i.pointer.middle_down());
    let pointer_delta = ui.input(|i| i.pointer.delta());
    if middle_down && pointer_delta != Vec2::ZERO {
        let delta = if app.invert_pan {
            pointer_delta
        } else {
            -pointer_delta
        };
        if app.mode == AppMode::Board {
            board_view::apply_pan(&mut app.board_view_offset, delta);
        } else {
            app.view_offset += delta;
        }
    }
    if app.mode == AppMode::Board {
        let space_held = ui.input(|i| i.key_down(egui::Key::Space));
        if space_held && ui.input(|i| i.pointer.primary_down()) && pointer_delta != Vec2::ZERO {
            let delta = if app.invert_pan {
                pointer_delta
            } else {
                -pointer_delta
            };
            board_view::apply_pan(&mut app.board_view_offset, delta);
        }
    }
}

/// 未拿起卡时滚轮缩放视口（会修改 `app` 内 offset/scale）。
pub(super) fn apply_scroll_zoom_when_not_picked(
    app: &mut DeckRefApp,
    ui: &mut egui::Ui,
    screen_center: Pos2,
    pointer: Option<Pos2>,
) {
    if app.picked_up.is_some() {
        return;
    }
    let scroll = ui.input(|i| i.raw_scroll_delta).y;
    if scroll == 0.0 {
        return;
    }
    if app.mode == AppMode::Board {
        if let Some(ptr) = pointer {
            board_view::apply_zoom_at_mouse(
                &mut app.board_view_offset,
                &mut app.board_view_scale,
                screen_center.to_vec2(),
                ptr.to_vec2(),
                scroll,
                super::VIEW_ZOOM_SENSITIVITY,
                super::VIEW_SCALE_MIN,
                super::VIEW_SCALE_MAX,
            );
        }
    } else {
        let world_before = pointer.map(|p| {
            let vt = ViewTransform::snapshot(app, screen_center, false);
            vt.screen_to_world(p).to_vec2()
        });
        app.view_scale = (app.view_scale * (1.0 + scroll * super::VIEW_ZOOM_SENSITIVITY))
            .clamp(super::VIEW_SCALE_MIN, super::VIEW_SCALE_MAX);
        if let (Some(ptr), Some(wb)) = (pointer, world_before) {
            let s_center = screen_center.to_vec2();
            app.view_offset = wb - (ptr.to_vec2() - s_center) / app.view_scale;
        }
    }
}

/// 每帧末尾写入 resize 补偿用的上一帧中心。
pub(super) fn save_screen_centers_for_next_frame(
    app: &mut DeckRefApp,
    board_screen_center_for_compensation: Vec2,
    screen_center: Pos2,
) {
    if app.mode == AppMode::Board {
        app.last_board_screen_center = Some(board_screen_center_for_compensation);
    } else {
        app.last_stack_screen_center = Some(screen_center.to_vec2());
    }
}
