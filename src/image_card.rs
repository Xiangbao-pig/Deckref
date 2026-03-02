//! 堆叠照片效果：ImageCard 结构体与渲染/交互逻辑。
//! 最新在前，后面的图片带偏移与旋转；支持悬停放大、长按置顶、按名称/大小/时间排序。

use eframe::egui::{self, emath::Rot2, Color32, Mesh, Pos2, Rect, Shape, Vec2};
use std::path::PathBuf;
use std::time::SystemTime;

/// 单张照片卡片：路径、内在尺寸、排序用元数据、悬停动画、随机布局偏移（散落效果）。
#[derive(Clone)]
pub struct ImageCard {
    pub path: PathBuf,
    pub intrinsic_size: Vec2,
    pub display_size: Vec2,
    pub file_name: String,
    pub file_size: u64,
    pub modified: SystemTime,
    pub anim_scale: f32,
    /// 基于路径哈希的随机位移（世界坐标），使堆叠更自然
    pub random_offset: Vec2,
    /// 基于路径哈希的随机旋转增量（弧度）
    pub random_rotation: f32,
}

impl ImageCard {
    /// 从路径哈希生成稳定随机数 [0, 1)，seed 用于得到不同序列
    fn path_hash_f32(path: &std::path::Path, seed: u64) -> f32 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut h = DefaultHasher::new();
        path.to_string_lossy().hash(&mut h);
        seed.hash(&mut h);
        let u = h.finish();
        (u % 10_000) as f32 / 10_000.0
    }

    /// 从路径与图片尺寸创建卡片；元数据读取失败时使用默认值。
    pub fn from_path(path: PathBuf, intrinsic_size: Vec2) -> Self {
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        let (file_size, modified) = std::fs::metadata(&path)
            .map(|m| (m.len(), m.modified().unwrap_or(SystemTime::UNIX_EPOCH)))
            .unwrap_or((0, SystemTime::UNIX_EPOCH));
        let max_side = intrinsic_size.x.max(intrinsic_size.y).max(1.0);
        let display_size = if max_side <= 0.0 {
            Vec2::new(220.0, 220.0)
        } else {
            intrinsic_size * (220.0 / max_side)
        };
        let (a, b, c) = (
            Self::path_hash_f32(&path, 0),
            Self::path_hash_f32(&path, 1),
            Self::path_hash_f32(&path, 2),
        );
        let random_offset = Vec2::new((a * 2.0 - 1.0) * 90.0, (b * 2.0 - 1.0) * 70.0);
        let random_rotation = (c * 2.0 - 1.0) * 0.18;
        Self {
            path,
            intrinsic_size,
            display_size,
            file_name,
            file_size,
            modified,
            anim_scale: 1.0,
            random_offset,
            random_rotation,
        }
    }
}

/// 排序方式（与 Windows 资源管理器常见选项一致）
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum CardSortOrder {
    #[default]
    /// 按文件名
    Name,
    /// 按文件大小
    Size,
    /// 按修改时间（新的在前）
    Modified,
}

/// 堆叠布局：在世界坐标中计算（中心 (0,0)），便于配合 view_offset 平移视角。
#[derive(Clone)]
pub struct StackLayout {
    /// 从后到前每张卡的位移增量（世界坐标）
    pub offset_per_index: Vec2,
    pub rotation_per_index: f32,
    pub base_rotation: f32,
    pub available_size: Vec2,
}

impl Default for StackLayout {
    fn default() -> Self {
        Self {
            offset_per_index: Vec2::new(28.0, 22.0),
            rotation_per_index: 0.048,
            base_rotation: -0.22,
            available_size: Vec2::new(600.0, 400.0),
        }
    }
}

impl StackLayout {
    /// 根据窗口与堆叠位置计算单张卡的显示尺寸：画面铺满，最前的图偏大。
    pub fn effective_display_size(&self, intrinsic_size: Vec2, index: usize, total: usize) -> Vec2 {
        if total == 0 {
            return Vec2::new(220.0, 220.0);
        }
        let min_side = self.available_size.x.min(self.available_size.y);
        let base_side = min_side * 0.45f32;
        let t = (index + 1) as f32 / total as f32;
        let stack_scale = 0.6 + 0.4 * t;
        let max_int = intrinsic_size.x.max(intrinsic_size.y).max(1.0);
        let scale = (base_side * stack_scale) / max_int;
        intrinsic_size * scale
    }

    fn layout_scale(&self) -> f32 {
        let ref_side = 600.0f32;
        let min_side = self.available_size.x.min(self.available_size.y);
        (min_side / ref_side).clamp(0.5, 2.0)
    }

    /// 计算第 index 张卡在世界坐标中的中心与旋转（不含每卡随机偏移/旋转）。
    pub fn position_and_rotation(&self, index: usize, total: usize) -> (Pos2, f32) {
        let s = self.layout_scale();
        let back_to_front = total.saturating_sub(1).saturating_sub(index);
        let offset = Vec2::new(
            back_to_front as f32 * self.offset_per_index.x * s,
            back_to_front as f32 * self.offset_per_index.y * s,
        );
        let world_pos = Pos2::new(offset.x, offset.y);
        let rot = self.base_rotation + back_to_front as f32 * self.rotation_per_index;
        (world_pos, rot)
    }
}

/// 用旋转矩形做点击/悬停检测：点是否在中心为 `center`、半尺寸 `half_size`、逆时针旋转 `angle` 的矩形内
pub fn rotated_rect_contains(center: Pos2, half_size: Vec2, angle: f32, point: Pos2) -> bool {
    let d = point - center;
    let rot = Rot2::from_angle(-angle);
    let local = rot * d;
    local.x >= -half_size.x && local.x <= half_size.x && local.y >= -half_size.y && local.y <= half_size.y
}

/// 绘制一张带旋转与缩放的纹理卡片
pub fn draw_card(
    painter: &egui::Painter,
    texture_id: egui::TextureId,
    center: Pos2,
    size: Vec2,
    rotation: f32,
    scale: f32,
    uv_rect: Rect,
) {
    let size = size * scale;
    let rect = Rect::from_center_size(center, size);
    let mut mesh = Mesh::with_texture(texture_id);
    mesh.add_rect_with_uv(rect, uv_rect, Color32::WHITE);
    mesh.rotate(Rot2::from_angle(rotation), center);
    painter.add(Shape::mesh(mesh));
}

/// 绘制卡片下方的轻微阴影（用于“拿起”状态），中心、尺寸、旋转与卡片一致，偏移在屏幕空间
pub fn draw_card_shadow(
    painter: &egui::Painter,
    center: Pos2,
    size: Vec2,
    rotation: f32,
    offset_screen: Vec2,
) {
    let shadow_center = center + offset_screen;
    let half = size * 0.51;
    let rot = Rot2::from_angle(rotation);
    let corners = [
        rot * Vec2::new(-half.x, -half.y) + shadow_center.to_vec2(),
        rot * Vec2::new(half.x, -half.y) + shadow_center.to_vec2(),
        rot * Vec2::new(half.x, half.y) + shadow_center.to_vec2(),
        rot * Vec2::new(-half.x, half.y) + shadow_center.to_vec2(),
    ];
    let color = Color32::from_black_alpha(42);
    let mut mesh = Mesh::default();
    for p in &corners {
        mesh.colored_vertex(Pos2::new(p.x, p.y), color);
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(Shape::mesh(mesh));
}

/// 动画缩放每帧向目标平滑逼近
pub const ANIM_SPEED: f32 = 8.0;

/// 拿起卡片时的目标缩放（轻微“浮起”感）
pub const PICKED_UP_SCALE: f32 = 1.12;

/// 拿起时在屏幕空间上的“抬升”偏移（向上为正），营造脱离堆叠的物理感
pub const PICKED_UP_LIFT: Vec2 = Vec2 { x: 0.0, y: -12.0 };

/// 拿起动画在约多少秒内完成（用于插值）
pub const PICK_UP_ANIM_DURATION: f32 = 0.12;

/// 跟手时的平滑系数 (0~1)，越大跟得越紧
pub const HELD_FOLLOW_SMOOTH: f32 = 0.35;

pub fn lerp_scale(current: f32, target: f32, dt: f32) -> f32 {
    let diff = target - current;
    if diff.abs() < 0.001 {
        return target;
    }
    current + diff * (1.0 - (-ANIM_SPEED * dt).exp())
}
