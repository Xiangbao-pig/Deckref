//! 画板模式视口逻辑（PS 式窗口 = 取景框）。
//!
//! 仅用于画板模式，与默认/灵感模式完全解耦。遵循 [BOARD_MODE_OPTIMIZATION.md](docs/BOARD_MODE_OPTIMIZATION.md)：
//! - 画布固定尺寸，元素位置与缩放为画布空间绝对值，**不**随窗口 Resize 缩放。
//! - 窗口变化时仅 **Clip/视口** 变化，不触发 Reflow/Repaint。
//! - 本模块仅提供视口变换与补偿的纯函数，状态由 `app` 持有。

use eframe::egui::Vec2;

/// 窗口 Resize 时补偿 offset，使原先在**旧中心**的世界点仍留在**旧中心**的屏幕位置（窗口只是取景器，拉大时看到更多画布，内容不跟边跑）。
/// 推导：old_center = new_center + scale*(offset_old - offset_new) => offset_new = offset_old + (old_center - new_center)/scale = offset_old + delta/scale。
#[inline]
pub fn apply_resize_compensation(
    offset: &mut Vec2,
    scale: f32,
    screen_center: Vec2,
    last_screen_center: Option<Vec2>,
) -> Vec2 {
    if let Some(prev) = last_screen_center {
        let delta = screen_center - prev;
        if delta.length_sq() > 1e-8 && scale > 1e-6 {
            *offset += delta / scale;
        }
    }
    screen_center
}

/// 以鼠标为中心缩放：缩放前后鼠标下的世界点保持在同一屏幕位置。
/// 公式：`offset += (pointer_screen - screen_center) * (1/s1 - 1/s2)`。
#[inline]
pub fn apply_zoom_at_mouse(
    offset: &mut Vec2,
    scale: &mut f32,
    screen_center: Vec2,
    pointer_screen: Vec2,
    scroll_delta: f32,
    zoom_sensitivity: f32,
    scale_min: f32,
    scale_max: f32,
) {
    let s1 = *scale;
    *scale = (s1 * (1.0 + scroll_delta * zoom_sensitivity)).clamp(scale_min, scale_max);
    let s2 = *scale;
    let rel = pointer_screen - screen_center;
    *offset += rel * (1.0 / s1 - 1.0 / s2);
}

/// 平移：将 delta 累加到 offset，不改变 scale。
#[inline]
pub fn apply_pan(offset: &mut Vec2, delta: Vec2) {
    *offset += delta;
}

/// 画板模式：从画布坐标和视口状态得到屏幕坐标。
/// 画布空间固定，不随窗口尺寸缩放；仅视口（offset, scale）与屏幕中心决定显示。
#[inline]
pub fn canvas_to_screen(canvas_pos: Vec2, screen_center: Vec2, offset: Vec2, scale: f32) -> Vec2 {
    let v = canvas_pos - offset;
    Vec2::new(screen_center.x + v.x * scale, screen_center.y + v.y * scale)
}

/// 画板模式：从屏幕坐标反算画布坐标。
#[inline]
pub fn screen_to_canvas(screen_pos: Vec2, screen_center: Vec2, offset: Vec2, scale: f32) -> Vec2 {
    let v = Vec2::new(screen_pos.x - screen_center.x, screen_pos.y - screen_center.y) / scale;
    offset + v
}
