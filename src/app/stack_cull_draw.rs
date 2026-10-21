//! 中央堆叠区：空状态、首帧阵列、卡片几何、命中、动画、视口剔除、纹理请求与绘制。

use super::DeckRefApp;
use super::stack_viewport::ViewTransform;
use crate::app_viewport::{screen_aabb_for_rotated_rect, CardScreenRect};
use crate::image_card::{
    draw_card, draw_card_shadow, lerp_position, lerp_rotation, lerp_scale, rotated_rect_contains,
    DISPLAY_SCALE_MAX, DISPLAY_SCALE_MIN, PICKED_UP_LIFT, PICKED_UP_SCALE, PICK_UP_ANIM_DURATION,
};
use crate::mode::AppMode;
use eframe::egui::{self, Pos2, Rect, Vec2};
use rstar::{AABB, Envelope, RTree};
use std::collections::HashSet;
use std::path::PathBuf;

pub(super) fn draw_empty_folder_message(ui: &mut egui::Ui, stack_rect: Rect, n: usize) {
    if n != 0 {
        return;
    }
    let msg = "当前文件夹中没有可以预览的图片";
    let gray = egui::Color32::from_rgba_unmultiplied(90, 90, 90, 240);
    let galley = ui
        .painter()
        .layout_no_wrap(msg.into(), egui::FontId::proportional(15.0), gray);
    let padding = 14.0;
    let frame_size = galley.size() + Vec2::splat(2.0 * padding);
    let frame_rect = Rect::from_center_size(stack_rect.center(), frame_size);
    ui.painter().rect_filled(
        frame_rect,
        egui::Rounding::same(6.0),
        egui::Color32::from_rgba_unmultiplied(250, 250, 250, 40),
    );
    ui.painter().rect_stroke(
        frame_rect,
        egui::Rounding::same(6.0),
        egui::Stroke::new(1.5, egui::Color32::from_rgba_unmultiplied(140, 140, 140, 220)),
    );
    let text_pos = frame_rect.center() - galley.size() / 2.0;
    ui.painter().galley(text_pos, galley, gray);
}

pub(super) fn maybe_initial_arrange(app: &mut DeckRefApp, stack_rect: Rect, n: usize) {
    if n > 0 && !app.initial_arrange_done {
        app.initial_arrange_done = true;
        app.stack_layout.available_size = stack_rect.size();
        if app.mode != AppMode::Board && app.grid_view {
            app.grid_arrange();
        } else if app.mode == AppMode::Board && app.grid_view && app.layout_reference_size.is_none() {
            app.grid_arrange();
        }
    }
}

pub(super) fn compute_card_display(app: &DeckRefApp, n: usize) -> Vec<(Vec2, f32, f32)> {
    let picked_up_index = app.picked_up.as_ref().map(|(i, _, _)| *i);
    (0..n)
        .map(|i| {
            let card = &app.cards[i];
            let (pos, scale_eff) = if app.mode == AppMode::Board {
                card.board_saved_world_pos
                    .map(|saved| {
                        (
                            saved + card.anim_position_delta,
                            card.display_scale
                                .clamp(DISPLAY_SCALE_MIN, DISPLAY_SCALE_MAX),
                        )
                    })
                    .unwrap_or_else(|| {
                        let (lp, _) = app.stack_layout.position_and_rotation(i, n);
                        (
                            lp.to_vec2() + card.random_offset + card.anim_position_delta,
                            card.display_scale,
                        )
                    })
            } else {
                let (lp, _) = app.stack_layout.position_and_rotation(i, n);
                (
                    lp.to_vec2() + card.random_offset + card.anim_position_delta,
                    card.display_scale,
                )
            };
            let pos = if Some(i) == picked_up_index && app.held_smooth_pos.is_some() {
                app.held_smooth_pos.unwrap_or(pos)
            } else {
                pos
            };
            let (_, rot) = app.stack_layout.position_and_rotation(i, n);
            let rot = if app.grid_view {
                0.0
            } else {
                rot + card.random_rotation + card.anim_rotation_offset
            };
            let scale_eff = if Some(i) == picked_up_index {
                app.held_scale.unwrap_or(scale_eff)
            } else {
                scale_eff
            };
            (pos, rot, scale_eff)
        })
        .collect()
}

pub(super) fn compute_hovered_index(
    app: &DeckRefApp,
    n: usize,
    card_display: &[(Vec2, f32, f32)],
    world_pointer: Option<Pos2>,
    is_board: bool,
    board_ref_size: Vec2,
) -> Option<usize> {
    let picked_up_index = app.picked_up.as_ref().map(|(i, _, _)| *i);
    for i in (0..n).rev() {
        if Some(i) == picked_up_index {
            continue;
        }
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
        if let Some(wp) = world_pointer {
            if rotated_rect_contains(
                Pos2::new(world_pos.x, world_pos.y),
                half,
                rot,
                wp,
            ) {
                return Some(i);
            }
        }
    }
    None
}

pub(super) fn update_card_animations(
    app: &mut DeckRefApp,
    dt: f32,
    hovered_index: Option<usize>,
) {
    let dropped_idx = app.recently_dropped_index;
    for (i, card) in app.cards.iter_mut().enumerate() {
        let hovered = hovered_index == Some(i);
        let target_scale = if hovered { 1.08 } else { 1.0 };
        card.anim_scale = lerp_scale(card.anim_scale, target_scale, dt);
        if dropped_idx == Some(i) {
            card.anim_rotation_offset = lerp_rotation(card.anim_rotation_offset, 0.0, dt);
        }
        card.anim_position_delta = lerp_position(card.anim_position_delta, Vec2::ZERO, dt);
    }
    if let Some(idx) = dropped_idx {
        if app.cards[idx].anim_rotation_offset.abs() < 1e-5 {
            app.recently_dropped_index = None;
        }
    }
}

pub(super) fn compute_visible_indices(
    app: &DeckRefApp,
    ui: &egui::Ui,
    n: usize,
    card_display: &[(Vec2, f32, f32)],
    view: &ViewTransform,
    is_board: bool,
    board_ref_size: Vec2,
) -> HashSet<usize> {
    const VISIBLE_RATIO_THRESHOLD: f64 = 0.2;
    const VIEWPORT_QUERY_PADDING: f32 = 500.0;
    let viewport_for_culling = ui.clip_rect().expand2(Vec2::splat(VIEWPORT_QUERY_PADDING));
    let viewport_aabb = AABB::from_corners(
        [
            viewport_for_culling.min.x as f64,
            viewport_for_culling.min.y as f64,
        ],
        [
            viewport_for_culling.max.x as f64,
            viewport_for_culling.max.y as f64,
        ],
    );
    let mut rects: Vec<CardScreenRect> = Vec::with_capacity(n);
    for (i, card) in app.cards.iter().enumerate() {
        let (world_pos, rot, scale_eff) = card_display[i];
        let world_center = Pos2::new(world_pos.x, world_pos.y);
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
        let aabb = screen_aabb_for_rotated_rect(world_center, half, rot, |p| view.world_to_screen(p));
        rects.push(CardScreenRect { index: i, aabb });
    }
    let tree = RTree::bulk_load(rects);
    tree.locate_in_envelope(&viewport_aabb)
        .filter_map(|r| {
            let card_area = r.aabb.area();
            if card_area <= 0.0 {
                return Some(r.index);
            }
            let inter = r.aabb.intersection_area(&viewport_aabb);
            if inter / card_area >= VISIBLE_RATIO_THRESHOLD {
                Some(r.index)
            } else {
                None
            }
        })
        .collect()
}

pub(super) fn request_textures_for_visible(
    app: &mut DeckRefApp,
    ctx: &egui::Context,
    n: usize,
    visible_indices: &HashSet<usize>,
) {
    let paths_to_load: Vec<PathBuf> = app
        .cards
        .iter()
        .rev()
        .enumerate()
        .filter_map(|(rev_i, c)| {
            let index = n - 1 - rev_i;
            if visible_indices.contains(&index) {
                Some(c.path.clone())
            } else {
                None
            }
        })
        .collect();
    for path in &paths_to_load {
        let _ = app.ensure_texture(ctx, path);
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_cards_marquee_and_held(
    app: &DeckRefApp,
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    n: usize,
    card_display: &[(Vec2, f32, f32)],
    view: &ViewTransform,
    visible_indices: &HashSet<usize>,
    is_board: bool,
    board_ref_size: Vec2,
    view_scale: f32,
) {
    let orig_clip = ui.clip_rect();
    ui.set_clip_rect(ui.ctx().screen_rect().expand2(Vec2::splat(5000.0)));
    let painter = ui.painter();
    let now = ctx.input(|i| i.time) as f32;
    for (i, card) in app.cards.iter().enumerate() {
        if app.picked_up.as_ref().map(|(idx, _, _)| *idx) == Some(i) {
            continue;
        }
        if !visible_indices.contains(&i) {
            continue;
        }
        let (world_pos, rot, scale_eff) = card_display[i];
        let screen_pos = view.world_to_screen(Pos2::new(world_pos.x, world_pos.y));
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
        let size = size * scale_eff * view_scale;
        if let Some(tex) = app.texture_cache.get(&card.path) {
            let uv = Rect::from_min_max([0.0, 0.0].into(), [1.0, 1.0].into());
            draw_card(
                painter,
                tex.id(),
                screen_pos,
                size,
                rot,
                card.anim_scale,
                uv,
            );
        }
    }
    if let Some((idx, _, pick_time)) = app.picked_up {
        if let Some(held_pos) = app.held_smooth_pos {
            let card = &app.cards[idx];
            let (_, rot) = app.stack_layout.position_and_rotation(idx, n);
            let rot = if app.grid_view {
                0.0
            } else {
                rot + card.random_rotation + card.anim_rotation_offset
            };
            let size = if is_board {
                crate::image_card::StackLayout::effective_display_size_in_area(
                    card.intrinsic_size,
                    idx,
                    n,
                    board_ref_size,
                )
            } else {
                app.stack_layout
                    .effective_display_size(card.intrinsic_size, idx, n)
            };
            let held_scale = app.held_scale.unwrap_or(1.0);
            let size = size * held_scale * view_scale;
            let screen_center = view.world_to_screen(Pos2::new(held_pos.x, held_pos.y));
            let t = ((now - pick_time as f32) / PICK_UP_ANIM_DURATION).min(1.0);
            let lift = PICKED_UP_LIFT * t;
            let scale = 1.0 + (PICKED_UP_SCALE - 1.0) * t;
            let shadow_offset = Vec2::new(6.0, 8.0);
            draw_card_shadow(
                painter,
                screen_center + lift,
                size,
                rot,
                shadow_offset,
            );
            if let Some(tex) = app.texture_cache.get(&card.path) {
                let uv = Rect::from_min_max([0.0, 0.0].into(), [1.0, 1.0].into());
                draw_card(
                    painter,
                    tex.id(),
                    screen_center + lift,
                    size,
                    rot,
                    scale,
                    uv,
                );
            }
        }
    }
    ui.set_clip_rect(orig_clip);

    let marquee_draw_area = ui.ctx().screen_rect();
    ui.set_clip_rect(marquee_draw_area);
    let painter = ui.painter();
    let screen_rect = ui.ctx().screen_rect();
    let pointer = ui.input(|i| i.pointer.hover_pos());
    for &i in &app.selected_indices {
        if i >= n {
            continue;
        }
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
        painter.rect_stroke(
            card_rect.expand(2.0),
            egui::Rounding::same(0.0),
            egui::Stroke::new(2.0, egui::Color32::from_rgb(60, 120, 220)),
        );
    }
    let marquee_effective_cur = pointer
        .or(app.marquee_last_end_screen)
        .map(|p| {
            Pos2::new(
                p.x.clamp(screen_rect.min.x, screen_rect.max.x),
                p.y.clamp(screen_rect.min.y, screen_rect.max.y),
            )
        });
    if let (Some(start), Some(cur)) = (app.marquee_start_screen, marquee_effective_cur) {
        let marquee_rect = Rect::from_min_max(
            Pos2::new(start.x.min(cur.x), start.y.min(cur.y)),
            Pos2::new(start.x.max(cur.x), start.y.max(cur.y)),
        );
        let draw_rect = marquee_rect.intersect(marquee_draw_area);
        if draw_rect.is_positive() {
            painter.rect_filled(
                draw_rect,
                0.0,
                egui::Color32::from_rgba_unmultiplied(60, 120, 220, 35),
            );
            painter.rect_stroke(
                draw_rect,
                egui::Rounding::same(0.0),
                egui::Stroke::new(1.5, egui::Color32::from_rgb(60, 120, 220)),
            );
        }
    }
    ui.set_clip_rect(orig_clip);
}
