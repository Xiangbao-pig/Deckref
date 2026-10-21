//! 中央堆叠画布：`stack_viewport` → 命中/剔除/绘制 → `stack_interaction`，顺序与拆分前一致。

use super::stack_cull_draw;
use super::stack_interaction;
use super::stack_viewport::{self, ViewTransform};
use super::{DeckRefApp, BOARD_DEFAULT_CANVAS_SIZE};
use crate::mode::AppMode;
use eframe::egui;

pub(super) fn show(app: &mut DeckRefApp, ctx: &egui::Context, ui: &mut egui::Ui, dt: f32) {
    let stack_rect = ui.available_rect_before_wrap();
    if !stack_rect.is_positive() {
        return;
    }
    app.last_stack_size = stack_rect.size();
    let screen_center = stack_rect.center();
    let board_screen_center_for_compensation = stack_viewport::board_screen_center_for_compensation(
        ui,
        screen_center,
        app.mode == AppMode::Board,
    );

    let n = app.cards.len();
    stack_cull_draw::draw_empty_folder_message(ui, stack_rect, n);
    stack_cull_draw::maybe_initial_arrange(app, stack_rect, n);

    stack_viewport::apply_resize_and_pan(app, ui, screen_center, board_screen_center_for_compensation);

    let pointer = ui.input(|i| i.pointer.hover_pos());
    let pointer_down = ui.input(|i| i.pointer.primary_down());
    let pointer_released = ui.input(|i| i.pointer.primary_released());
    let screen_rect = ui.ctx().screen_rect();

    stack_interaction::update_marquee_endpoints(app, pointer);

    let is_board = app.mode == AppMode::Board;
    let board_ref_size = app.layout_reference_size.unwrap_or(BOARD_DEFAULT_CANVAS_SIZE);

    // 与本帧命中/绘制一致：滚轮缩放写入 `app` 之前快照视口（与原先闭包捕获行为一致）。
    let view = ViewTransform::snapshot(app, screen_center, is_board);
    stack_viewport::apply_scroll_zoom_when_not_picked(app, ui, screen_center, pointer);

    let card_display = stack_cull_draw::compute_card_display(app, n);
    let world_pointer = pointer.map(|p| view.screen_to_world(p));
    let hovered_index = stack_cull_draw::compute_hovered_index(
        app,
        n,
        &card_display,
        world_pointer,
        is_board,
        board_ref_size,
    );
    stack_cull_draw::update_card_animations(app, dt, hovered_index);

    let visible_indices = stack_cull_draw::compute_visible_indices(
        app,
        ui,
        n,
        &card_display,
        &view,
        is_board,
        board_ref_size,
    );
    stack_cull_draw::request_textures_for_visible(app, ctx, n, &visible_indices);
    stack_cull_draw::draw_cards_marquee_and_held(
        app,
        ui,
        ctx,
        n,
        &card_display,
        &view,
        &visible_indices,
        is_board,
        board_ref_size,
        view.view_scale(),
    );

    stack_interaction::picked_up_frame_updates(app, ctx, ui, &card_display, world_pointer);
    stack_interaction::pointer_finish_select(
        app,
        ctx,
        n,
        &card_display,
        &view,
        is_board,
        board_ref_size,
        hovered_index,
        pointer,
        pointer_down,
        pointer_released,
        screen_rect,
    );

    stack_viewport::save_screen_centers_for_next_frame(
        app,
        board_screen_center_for_compensation,
        screen_center,
    );
}
