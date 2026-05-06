//! 堆叠区视口剔除：卡片屏幕空间 AABB 与 R-Tree 条目。

use eframe::egui::{self, emath::Rot2, Vec2};
use rstar::{AABB, RTreeObject};

/// 用于视口剔除：卡片在屏幕空间的 AABB + 索引。先经 R-Tree 查询与视口相交的卡，再对查询结果做可见比例判断。
pub(crate) struct CardScreenRect {
    pub(crate) index: usize,
    pub(crate) aabb: AABB<[f64; 2]>,
}

impl RTreeObject for CardScreenRect {
    type Envelope = AABB<[f64; 2]>;
    fn envelope(&self) -> Self::Envelope {
        self.aabb
    }
}

/// 由世界坐标中心、半尺寸、旋转角与 world_to_screen 得到屏幕空间 AABB（用于可见比例计算）。
pub(crate) fn screen_aabb_for_rotated_rect(
    world_center: egui::Pos2,
    half_size: egui::Vec2,
    rot: f32,
    world_to_screen: impl Fn(egui::Pos2) -> egui::Pos2,
) -> AABB<[f64; 2]> {
    let r = Rot2::from_angle(rot);
    let corners = [
        world_center + r * Vec2::new(half_size.x, half_size.y),
        world_center + r * Vec2::new(-half_size.x, half_size.y),
        world_center + r * Vec2::new(-half_size.x, -half_size.y),
        world_center + r * Vec2::new(half_size.x, -half_size.y),
    ];
    let screen: Vec<egui::Pos2> = corners.iter().map(|&p| world_to_screen(p)).collect();
    let min_x = screen.iter().map(|p| p.x).fold(f32::INFINITY, f32::min);
    let min_y = screen.iter().map(|p| p.y).fold(f32::INFINITY, f32::min);
    let max_x = screen.iter().map(|p| p.x).fold(f32::NEG_INFINITY, f32::max);
    let max_y = screen.iter().map(|p| p.y).fold(f32::NEG_INFINITY, f32::max);
    AABB::from_corners([min_x as f64, min_y as f64], [max_x as f64, max_y as f64])
}
