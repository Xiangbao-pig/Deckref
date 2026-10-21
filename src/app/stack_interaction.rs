//! 中央堆叠区：框选端点、长按拿起、跟手、松手放下与选中逻辑。

use super::stack_viewport::ViewTransform;
use super::DeckRefApp;
use crate::app_viewport::screen_aabb_for_rotated_rect;
use crate::image_card::{
    normalize_angle_delta, DISPLAY_SCALE_MAX, DISPLAY_SCALE_MIN, HELD_FOLLOW_SMOOTH,
    WHEEL_SCALE_SENSITIVITY,
};
use crate::mode::AppMode;
use eframe::egui::{self, Pos2, Rect, Vec2};
use std::collections::HashSet;

pub(super) fn update_marquee_endpoints(app: &mut DeckRefApp, pointer: Option<Pos2>) {
    if app.marquee_start_screen.is_some() {
        if let Some(p) = pointer {
            app.marquee_last_end_screen = Some(p);
        }
    }
}

/// 绘制完成之后：长按超时进入拿起、拿起时滚轮缩放、跟手插值（须使用与本帧绘制相同的 `world_pointer`，即滚轮缩放**之前**的视口变换）。
pub(super) fn picked_up_frame_updates(
    app: &mut DeckRefApp,
    ctx: &egui::Context,
    ui: &egui::Ui,
    card_display: &[(Vec2, f32, f32)],
    world_pointer: Option<Pos2>,
) {
    if app.picked_up.is_none() && app.long_press.is_some() {
        if let Some((idx, start)) = app.long_press {
            if ctx.input(|i| i.time) - start >= super::LONG_PRESS_DURATION as f64 {
                let (world_pos, _, _) = card_display[idx];
                let card_world = Pos2::new(world_pos.x, world_pos.y);
                let grab_offset_world = world_pointer
                    .map(|wp| wp.to_vec2() - card_world.to_vec2())
                    .unwrap_or(Vec2::ZERO);
                let now = ctx.input(|i| i.time);
                app.picked_up = Some((idx, grab_offset_world, now));
                app.held_smooth_pos = Some(card_world.to_vec2());
                app.held_scale = Some(card_display[idx].2);
                app.long_press = None;
                if app.allow_drag_out {
                    app.drag_pending = Some(idx);
                }
            }
        }
    }

    if let Some((_idx, _, _)) = app.picked_up {
        let scroll = ui.input(|i| i.raw_scroll_delta).y;
        if scroll != 0.0 {
            let current = app.held_scale.get_or_insert(1.0);
            *current = (*current * (1.0 + scroll * WHEEL_SCALE_SENSITIVITY))
                .clamp(DISPLAY_SCALE_MIN, DISPLAY_SCALE_MAX);
        }
    }

    if let Some((_idx, grab_offset_world, _)) = app.picked_up {
        let target_world = world_pointer
            .map(|wp| wp.to_vec2() - grab_offset_world)
            .unwrap_or_else(|| app.held_smooth_pos.unwrap_or(Vec2::ZERO));
        let current = app.held_smooth_pos.get_or_insert(target_world);
        *current = *current + (target_world - *current) * HELD_FOLLOW_SMOOTH;
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn pointer_finish_select(
    app: &mut DeckRefApp,
    ctx: &egui::Context,
    n: usize,
    card_display: &[(Vec2, f32, f32)],
    view: &ViewTransform,
    is_board: bool,
    board_ref_size: Vec2,
    hovered_index: Option<usize>,
    pointer: Option<Pos2>,
    pointer_down: bool,
    pointer_released: bool,
    screen_rect: Rect,
) {
    let world_pointer = pointer.map(|p| view.screen_to_world(p));

    if pointer_released {
        if let Some((idx, grab_offset_world, _)) = app.picked_up.take() {
            app.held_smooth_pos = None;
            let scale = app.held_scale.unwrap_or(1.0);
            app.held_scale = None;
            let drop_world = world_pointer
                .map(|wp| wp.to_vec2() - grab_offset_world)
                .unwrap_or(Vec2::ZERO);
            let in_board = app.mode == AppMode::Board && app.layout_reference_size.is_some();
            let (layout_front, new_layout_rot) = app.stack_layout.position_and_rotation(n.saturating_sub(1), n);
            let (_, old_layout_rot) = app.stack_layout.position_and_rotation(idx, n);
            let new_offset = drop_world - layout_front.to_vec2();
            let current_total_rot = if let Some(card) = app.cards.get(idx) {
                old_layout_rot + card.random_rotation + card.anim_rotation_offset
            } else {
                old_layout_rot - new_layout_rot
            };
            let old_world_pos: Vec<Vec2> = (0..n)
                .map(|i| {
                    let (pos, _, _) = card_display[i];
                    pos
                })
                .collect();
            let old_layout_rots: Vec<f32> = (0..n)
                .map(|i| app.stack_layout.position_and_rotation(i, n).1)
                .collect();
            let old_total_rots: Vec<f32> = (0..n)
                .map(|i| {
                    old_layout_rots[i]
                        + app.cards[i].random_rotation
                        + app.cards[i].anim_rotation_offset
                })
                .collect();
            if let Some(card) = app.cards.get_mut(idx) {
                let board_ref_sz = app.layout_reference_size.unwrap_or(app.last_stack_size);
                let old_effective = if in_board {
                    crate::image_card::StackLayout::effective_display_size_in_area(
                        card.intrinsic_size,
                        idx,
                        n,
                        board_ref_sz,
                    )
                } else {
                    app.stack_layout
                        .effective_display_size(card.intrinsic_size, idx, n)
                };
                let new_effective = if in_board {
                    crate::image_card::StackLayout::effective_display_size_in_area(
                        card.intrinsic_size,
                        n - 1,
                        n,
                        board_ref_sz,
                    )
                } else {
                    app.stack_layout
                        .effective_display_size(card.intrinsic_size, n - 1, n)
                };
                let new_scale = if new_effective.x > 1e-6 && new_effective.y > 1e-6 {
                    let sx = scale * old_effective.x / new_effective.x;
                    let sy = scale * old_effective.y / new_effective.y;
                    sx.min(sy).clamp(DISPLAY_SCALE_MIN, DISPLAY_SCALE_MAX)
                } else {
                    scale.clamp(DISPLAY_SCALE_MIN, DISPLAY_SCALE_MAX)
                };
                if in_board {
                    card.board_saved_world_pos = Some(drop_world);
                    card.random_offset = drop_world;
                    card.display_scale = new_scale;
                } else {
                    card.random_offset = new_offset;
                    card.display_scale = new_scale;
                }
                if app.grid_view {
                    card.random_rotation = 0.0;
                    card.anim_rotation_offset = 0.0;
                } else {
                    card.random_rotation = -new_layout_rot;
                    card.anim_rotation_offset = normalize_angle_delta(current_total_rot);
                }
            }
            app.bring_to_front(idx);
            let board_ref_sz = app.layout_reference_size.unwrap_or(app.last_stack_size);
            for i in idx..(n - 1) {
                let (new_lp, new_layout_rot) = app.stack_layout.position_and_rotation(i, n);
                let new_lp = new_lp.to_vec2();
                if in_board {
                    app.cards[i].board_saved_world_pos = Some(old_world_pos[i + 1]);
                    app.cards[i].random_offset = old_world_pos[i + 1];
                } else {
                    app.cards[i].random_offset = old_world_pos[i + 1] - new_lp;
                }
                app.cards[i].anim_position_delta = Vec2::ZERO;
                if app.grid_view {
                    app.cards[i].anim_rotation_offset = 0.0;
                } else {
                    app.cards[i].anim_rotation_offset = normalize_angle_delta(
                        old_total_rots[i + 1] - new_layout_rot - app.cards[i].random_rotation,
                    );
                }
                let card = &mut app.cards[i];
                let old_eff = if in_board {
                    crate::image_card::StackLayout::effective_display_size_in_area(
                        card.intrinsic_size,
                        i + 1,
                        n,
                        board_ref_sz,
                    )
                } else {
                    app.stack_layout
                        .effective_display_size(card.intrinsic_size, i + 1, n)
                };
                let new_eff = if in_board {
                    crate::image_card::StackLayout::effective_display_size_in_area(
                        card.intrinsic_size,
                        i,
                        n,
                        board_ref_sz,
                    )
                } else {
                    app.stack_layout
                        .effective_display_size(card.intrinsic_size, i, n)
                };
                if new_eff.x > 1e-6 && new_eff.y > 1e-6 {
                    let ratio_x = old_eff.x / new_eff.x;
                    let ratio_y = old_eff.y / new_eff.y;
                    let ratio = ratio_x.min(ratio_y);
                    card.display_scale = (card.display_scale * ratio).clamp(DISPLAY_SCALE_MIN, DISPLAY_SCALE_MAX);
                }
            }
            app.recently_dropped_index = Some(n - 1);
        }
        app.long_press = None;
        app.drag_pending = None;
        if app.marquee_start_screen.is_some() {
            let start = app.marquee_start_screen.take().unwrap();
            let last_end = app.marquee_last_end_screen.take();
            let effective_cur = pointer.or(last_end).map(|p| {
                Pos2::new(
                    p.x.clamp(screen_rect.min.x, screen_rect.max.x),
                    p.y.clamp(screen_rect.min.y, screen_rect.max.y),
                )
            });
            if let Some(cur) = effective_cur {
                let marquee_rect = Rect::from_min_max(
                    Pos2::new(start.x.min(cur.x), start.y.min(cur.y)),
                    Pos2::new(start.x.max(cur.x), start.y.max(cur.y)),
                );
                let left_to_right = cur.x >= start.x;
                let use_contain = app.autodesk_style_selection && left_to_right;
                let mut sel = HashSet::new();
                for i in 0..n {
                    let (world_pos, rot, scale_eff) = card_display[i];
                    let card = &app.cards[i];
                    let size = if is_board {
                        crate::image_card::StackLayout::effective_display_size_in_area(
                            card.intrinsic_size,
                            i,
                            n,
                            board_ref_size,
                        )
                    } else {
                        app.stack_layout
                            .effective_display_size(card.intrinsic_size, i, n)
                    };
                    let size = size * scale_eff;
                    let half = size / 2.0;
                    let world_center = Pos2::new(world_pos.x, world_pos.y);
                    let aabb = screen_aabb_for_rotated_rect(world_center, half, rot, |p| view.world_to_screen(p));
                    let card_rect = Rect::from_min_max(
                        Pos2::new(aabb.lower()[0] as f32, aabb.lower()[1] as f32),
                        Pos2::new(aabb.upper()[0] as f32, aabb.upper()[1] as f32),
                    );
                    let hit = if use_contain {
                        marquee_rect.contains_rect(card_rect)
                    } else {
                        marquee_rect.intersects(card_rect)
                    };
                    if hit {
                        sel.insert(i);
                    }
                }
                app.selected_indices = sel;
            } else {
                app.selected_indices.clear();
            }
        } else if let Some(idx) = hovered_index {
            app.selected_indices.clear();
            app.selected_indices.insert(idx);
        } else {
            app.selected_indices.clear();
        }
    } else if pointer_down {
        if app.picked_up.is_none() {
            if let Some(idx) = hovered_index {
                if app.long_press.is_none() && app.marquee_start_screen.is_none() {
                    let space_held = app.mode == AppMode::Board && ctx.input(|i| i.key_down(egui::Key::Space));
                    if !space_held {
                        app.long_press = Some((idx, ctx.input(|i| i.time)));
                    }
                }
            } else {
                app.long_press = None;
                app.drag_pending = None;
                let space_held = app.mode == AppMode::Board && ctx.input(|i| i.key_down(egui::Key::Space));
                if !space_held && app.marquee_start_screen.is_none() {
                    app.marquee_start_screen = pointer;
                    app.marquee_last_end_screen = pointer;
                }
            }
        }
    } else if hovered_index.is_none() && app.picked_up.is_none() {
        app.long_press = None;
        app.drag_pending = None;
    }
}
