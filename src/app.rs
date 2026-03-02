//! UI 逻辑：堆叠照片、纹理缓存、悬停/长按/排序。

use crate::error::AppResult;
use crate::image_card::{
    draw_card, draw_card_shadow, lerp_scale, rotated_rect_contains, CardSortOrder, ImageCard,
    StackLayout, HELD_FOLLOW_SMOOTH, PICKED_UP_LIFT, PICKED_UP_SCALE, PICK_UP_ANIM_DURATION,
};
use crate::image_ops;
use crate::watcher::{list_existing_image_paths, WatchEvent, WatcherConfig};
use eframe::egui::{self, ColorImage, Rect, TextureHandle, Vec2};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;

const MAX_CARDS: usize = 80;
const LONG_PRESS_DURATION: f32 = 0.5;

/// 拿起状态：卡片索引 + 抓取时指针相对卡片中心的世界坐标偏移（用于跟手）
/// 以及拿起时刻的时间（用于拿起动画插值）
type PickedUpState = (usize, egui::Vec2, f64);

/// 应用状态：监听线程 + 卡片堆叠 + 纹理缓存 + 视角平移。
pub struct DeckRefApp {
    watch_rx: mpsc::Receiver<WatchEvent>,
    _watcher_handle: Option<thread::JoinHandle<AppResult<()>>>,
    cards: Vec<ImageCard>,
    texture_cache: std::collections::HashMap<PathBuf, TextureHandle>,
    watch_path: String,
    stack_layout: StackLayout,
    sort_order: CardSortOrder,
    long_press: Option<(usize, f64)>,
    /// 长按后进入“拿起”状态；松手时放下并更新位置/顺序
    picked_up: Option<PickedUpState>,
    /// 用于跟手的平滑位置（世界坐标），每帧插值向目标靠近
    held_smooth_pos: Option<egui::Vec2>,
    drag_pending: Option<usize>,
    /// 视角平移（中键拖拽），世界坐标偏移
    view_offset: egui::Vec2,
}

impl DeckRefApp {
    pub fn new(watch_path: PathBuf) -> AppResult<Self> {
        let (tx, rx) = mpsc::channel();
        let config = WatcherConfig {
            watch_path: watch_path.clone(),
            ..WatcherConfig::default()
        };
        let handle = crate::watcher::run_watcher(config, tx)?;
        let mut app = Self {
            watch_rx: rx,
            _watcher_handle: Some(handle),
            cards: Vec::new(),
            texture_cache: std::collections::HashMap::new(),
            watch_path: watch_path.display().to_string(),
            stack_layout: StackLayout::default(),
            sort_order: CardSortOrder::default(),
            long_press: None,
            picked_up: None,
            held_smooth_pos: None,
            drag_pending: None,
            view_offset: egui::Vec2::ZERO,
        };
        app.load_existing_images();
        Ok(app)
    }

    /// 启动时扫描目录，将已有图片加入列表（按修改时间新→旧）。
    fn load_existing_images(&mut self) {
        let watch_path = std::path::PathBuf::from(&self.watch_path);
        let paths = match list_existing_image_paths(&watch_path) {
            Ok(p) => p,
            Err(_) => return,
        };
        for path in paths {
            if self.cards.len() >= MAX_CARDS {
                break;
            }
            let (w, h) = match image_ops::image_dimensions(&path) {
                Ok(d) => d,
                Err(_) => continue,
            };
            let intrinsic = Vec2::new(w as f32, h as f32);
            let card = ImageCard::from_path(path, intrinsic);
            self.cards.push(card);
        }
    }

    fn poll_watch_events(&mut self) {
        while let Ok(ev) = self.watch_rx.try_recv() {
            let path = match ev {
                WatchEvent::Create(p) | WatchEvent::Modify(p) | WatchEvent::Remove(p) => p,
            };
            let display_size = image_ops::image_dimensions(&path)
                .map(|(w, h)| Vec2::new(w as f32, h as f32))
                .unwrap_or(Vec2::new(220.0, 220.0));
            let card = ImageCard::from_path(path, display_size);
            self.cards.push(card);
            if self.cards.len() > MAX_CARDS {
                let removed = self.cards.remove(0);
                self.texture_cache.remove(&removed.path);
            }
        }
    }

    /// 确保纹理已加载；若失败则返回 None，不 panic。
    fn ensure_texture(&mut self, ctx: &egui::Context, path: &PathBuf) -> Option<&TextureHandle> {
        if self.texture_cache.contains_key(path) {
            return self.texture_cache.get(path);
        }
        let img = image_ops::load_image(path).ok()?;
        let size = [img.width() as usize, img.height() as usize];
        let rgba = img.to_rgba8();
        let color_image = ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
        let name = path.to_string_lossy().to_string();
        let handle = ctx.load_texture(name, color_image, Default::default());
        self.texture_cache.insert(path.clone(), handle);
        self.texture_cache.get(path)
    }

    /// 按当前排序方式重排卡片（顺序 = 从后到前）
    fn apply_sort(&mut self) {
        match self.sort_order {
            CardSortOrder::Name => self.cards.sort_by(|a, b| a.file_name.cmp(&b.file_name)),
            CardSortOrder::Size => self.cards.sort_by(|a, b| a.file_size.cmp(&b.file_size)),
            CardSortOrder::Modified => self
                .cards
                .sort_by(|a, b| b.modified.cmp(&a.modified)), // 新的在前
        }
    }

    /// 将索引为 `index` 的卡片移到最前（列表末尾）
    fn bring_to_front(&mut self, index: usize) {
        if index >= self.cards.len() {
            return;
        }
        let card = self.cards.remove(index);
        self.cards.push(card);
    }

    fn run_ui(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.poll_watch_events();
        let dt = ctx.input(|i| i.stable_dt).min(0.1);

        // 堆叠区域
        egui::CentralPanel::default()
            .frame(egui::Frame::none())
            .show(ctx, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label("DeckRef");
                    ui.separator();
                    ui.label(format!("监听: {}", self.watch_path));
                    ui.add_space(8.0);
                    if ui.button("按名称").clicked() {
                        self.sort_order = CardSortOrder::Name;
                        self.apply_sort();
                    }
                    if ui.button("按大小").clicked() {
                        self.sort_order = CardSortOrder::Size;
                        self.apply_sort();
                    }
                    if ui.button("按修改时间").clicked() {
                        self.sort_order = CardSortOrder::Modified;
                        self.apply_sort();
                    }
                    if ui.button("清空").clicked() {
                        self.cards.clear();
                        self.texture_cache.clear();
                    }
                });
                ui.add_space(4.0);

                let stack_rect = ui.available_rect_before_wrap();
                if stack_rect.is_positive() {
                    self.stack_layout.available_size = stack_rect.size();
                    let screen_center = stack_rect.center();
                    let n = self.cards.len();

                    // 中键拖拽平移视角（类似 Autodesk）
                    let middle_down = ui.input(|i| i.pointer.middle_down());
                    let pointer_delta = ui.input(|i| i.pointer.delta());
                    if middle_down && pointer_delta != egui::Vec2::ZERO {
                        self.view_offset += pointer_delta;
                    }

                    let pointer = ui.input(|i| i.pointer.hover_pos());
                    let pointer_down = ui.input(|i| i.pointer.primary_down());
                    let pointer_released = ui.input(|i| i.pointer.primary_released());

                    let view_offset = self.view_offset;
                    let world_to_screen = |w: egui::Pos2| egui::Pos2::new(
                        screen_center.x + w.x - view_offset.x,
                        screen_center.y + w.y - view_offset.y,
                    );
                    let screen_to_world = |s: egui::Pos2| egui::Pos2::new(
                        s.x - screen_center.x + view_offset.x,
                        s.y - screen_center.y + view_offset.y,
                    );

                    // 命中检测（从最前到最后），使用世界坐标；拿起中的卡片不参与悬停
                    let picked_up_index = self.picked_up.as_ref().map(|(i, _, _)| *i);
                    let mut hovered_index = None;
                    let world_pointer = pointer.map(screen_to_world);
                    for i in (0..n).rev() {
                        if Some(i) == picked_up_index {
                            continue;
                        }
                        let card = &self.cards[i];
                        let (world_pos, rot) = self.stack_layout.position_and_rotation(i, n);
                        let world_pos = world_pos + card.random_offset;
                        let rot = rot + card.random_rotation;
                        let size = self.stack_layout.effective_display_size(card.intrinsic_size, i, n);
                        let half = size / 2.0;
                        if let Some(wp) = world_pointer {
                            if rotated_rect_contains(world_pos, half, rot, wp) {
                                hovered_index = Some(i);
                                break;
                            }
                        }
                    }

                    // 更新每张卡的 anim_scale：仅悬停时稍微放大，不移动位置
                    for (i, card) in self.cards.iter_mut().enumerate() {
                        let hovered = hovered_index == Some(i);
                        let target_scale = if hovered { 1.08 } else { 1.0 }; // 稍微放大
                        card.anim_scale = lerp_scale(card.anim_scale, target_scale, dt);
                    }

                    // 先复制路径，再预加载纹理，避免在迭代时同时可变借用 self
                    let paths: Vec<PathBuf> = self.cards.iter().map(|c| c.path.clone()).collect();
                    for path in &paths {
                        let _ = self.ensure_texture(ctx, path);
                    }

                    // 绘制：世界坐标 + 随机偏移，再转屏幕坐标；扩大裁剪区使图片可超出面板
                    let orig_clip = ui.clip_rect();
                    ui.set_clip_rect(ui.ctx().screen_rect().expand2(egui::Vec2::splat(5000.0)));
                    let painter = ui.painter();
                    let now = ctx.input(|i| i.time) as f32;
                    for (i, card) in self.cards.iter().enumerate() {
                        if self.picked_up.as_ref().map(|(idx, _, _)| *idx) == Some(i) {
                            continue;
                        }
                        let (world_pos, rot) = self.stack_layout.position_and_rotation(i, n);
                        let world_pos = world_pos + card.random_offset;
                        let rot = rot + card.random_rotation;
                        let screen_pos = world_to_screen(world_pos);
                        let size = self.stack_layout.effective_display_size(card.intrinsic_size, i, n);
                        if let Some(tex) = self.texture_cache.get(&card.path) {
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
                    // 拿起中的卡片：最后绘制（在最上层），带阴影、抬升与缩放
                    if let Some((idx, _, pick_time)) = self.picked_up {
                        if let Some(held_pos) = self.held_smooth_pos {
                            let card = &self.cards[idx];
                            let (_, rot) = self.stack_layout.position_and_rotation(idx, n);
                            let rot = rot + card.random_rotation;
                            let size = self.stack_layout.effective_display_size(card.intrinsic_size, idx, n);
                            let screen_center = world_to_screen(egui::Pos2::new(held_pos.x, held_pos.y));
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
                            if let Some(tex) = self.texture_cache.get(&card.path) {
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

                    // 长按超时 → 进入“拿起”状态（卡片将跟手，松手时放下并更新位置/顺序）
                    if self.picked_up.is_none() && self.long_press.is_some() {
                        if let Some((idx, start)) = self.long_press {
                            if ctx.input(|i| i.time) - start >= LONG_PRESS_DURATION as f64 {
                                let (world_pos, _rot) = self.stack_layout.position_and_rotation(idx, n);
                                let card = &self.cards[idx];
                                let card_world = world_pos + card.random_offset;
                                let grab_offset_world = world_pointer
                                    .map(|wp| wp.to_vec2() - card_world.to_vec2())
                                    .unwrap_or(egui::Vec2::ZERO);
                                let now = ctx.input(|i| i.time);
                                self.picked_up = Some((idx, grab_offset_world, now));
                                self.held_smooth_pos = Some(card_world.to_vec2());
                                self.long_press = None;
                                self.drag_pending = None;
                            }
                        }
                    }

                    // 拿起时：跟手平滑（轻微物理感）
                    if let Some((_idx, grab_offset_world, _)) = self.picked_up {
                        let target_world = world_pointer
                            .map(|wp| wp.to_vec2() - grab_offset_world)
                            .unwrap_or_else(|| self.held_smooth_pos.unwrap_or(egui::Vec2::ZERO));
                        let current = self.held_smooth_pos.get_or_insert(target_world);
                        *current = *current + (target_world - *current) * HELD_FOLLOW_SMOOTH;
                    }

                    // 松手：若正在拿起某张卡则“放下”（更新位置并置顶），并清除拿起状态
                    if pointer_released {
                        if let Some((idx, grab_offset_world, _)) = self.picked_up.take() {
                            self.held_smooth_pos = None;
                            let drop_world = world_pointer
                                .map(|wp| wp.to_vec2() - grab_offset_world)
                                .unwrap_or(egui::Vec2::ZERO);
                            // 放下后卡片会置顶，故偏移应相对“最前一张”的布局位置
                            let (layout_front, _) = self.stack_layout.position_and_rotation(n.saturating_sub(1), n);
                            let new_offset = drop_world - layout_front.to_vec2();
                            if let Some(card) = self.cards.get_mut(idx) {
                                card.random_offset = new_offset;
                            }
                            self.bring_to_front(idx);
                        }
                        self.long_press = None;
                        self.drag_pending = None;
                    } else if pointer_down {
                        if self.picked_up.is_none() {
                            if let Some(idx) = hovered_index {
                                if self.long_press.is_none() {
                                    self.long_press = Some((idx, ctx.input(|i| i.time)));
                                }
                            } else {
                                self.long_press = None;
                                self.drag_pending = None;
                            }
                        }
                    } else if hovered_index.is_none() && self.picked_up.is_none() {
                        self.long_press = None;
                        self.drag_pending = None;
                    }
                }
            });

        // 长按后移动鼠标：发起文件拖放到资源管理器/其他软件（阻塞至拖放结束）
        if let Some(idx) = self.drag_pending {
            let delta = ctx.input(|i| i.pointer.delta());
            if delta.length_sq() > 0.0001 {
                if let Some(card) = self.cards.get(idx) {
                    let path = card.path.clone();
                    let canonical = std::fs::canonicalize(&path).ok();
                    let paths = canonical
                        .as_ref()
                        .map(|p| vec![p.clone()])
                        .unwrap_or_else(|| vec![path.clone()]);
                    #[cfg(not(target_os = "linux"))]
                    {
                        let item = drag::DragItem::Files(paths);
                        let preview = drag::Image::File(path);
                        let _ = drag::start_drag(
                            frame,
                            item,
                            preview,
                            |_result, _cursor| {},
                            drag::Options::default(),
                        );
                    }
                }
                self.drag_pending = None;
                self.long_press = None;
            }
        }

        ctx.request_repaint();
    }
}

impl eframe::App for DeckRefApp {
    /// 半透明背景（液态磨砂风格 fallback：浅色 + 透明度）
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        egui::Rgba::from_rgba_unmultiplied(0.95, 0.95, 1.0, 0.52).to_array()
    }

    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.run_ui(ctx, frame);
    }
}
