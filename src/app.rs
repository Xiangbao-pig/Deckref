//! UI 逻辑：堆叠照片、纹理缓存、悬停/长按/排序。

use crate::error::AppResult;
use crate::image_card::{
    draw_card, draw_card_shadow, lerp_position, lerp_rotation, lerp_scale, normalize_angle_delta,
    rotated_rect_contains, CardSortOrder, ImageCard, StackLayout, DISPLAY_SCALE_MAX,
    DISPLAY_SCALE_MIN, DROP_ANIM_INITIAL_OFFSET_Y, DROP_ANIM_INITIAL_SCALE, HELD_FOLLOW_SMOOTH,
    PICKED_UP_LIFT, PICKED_UP_SCALE, PICK_UP_ANIM_DURATION, WHEEL_SCALE_SENSITIVITY,
};
use crate::image_ops;
use crate::watcher::{list_existing_image_paths, WatchEvent, WatcherConfig};
use eframe::egui::{self, ColorImage, Rect, TextureHandle, Vec2};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;

const MAX_CARDS: usize = 80;
const LONG_PRESS_DURATION: f32 = 0.5;
/// 底部栏高度（单行工具栏，不抢占主内容区；桌面端鼠标点击足够）
const BOTTOM_BAR_HEIGHT: f32 = 36.0;
/// 视图缩放下限（移远），50%
const VIEW_SCALE_MIN: f32 = 0.5;
/// 视图缩放上限（移近），250%
const VIEW_SCALE_MAX: f32 = 2.5;
/// 未拿起图片时滚轮调节视图缩放的灵敏度
const VIEW_ZOOM_SENSITIVITY: f32 = 0.0012;

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
    /// 拿起时滚轮调节的缩放（30%～500%），放下时写回卡片的 display_scale
    held_scale: Option<f32>,
    drag_pending: Option<usize>,
    /// 视角平移（中键拖拽），世界坐标偏移
    view_offset: egui::Vec2,
    /// 视图缩放（内容移近/移远），1.0 = 100%；未拿起图片时滚轮调节，一览时重置为 1.0
    view_scale: f32,
    /// 设置面板是否打开
    settings_open: bool,
    /// 反转中键平移方向（为 true 时与默认相反，用于适应不同习惯）
    invert_pan: bool,
    /// 本帧或上一帧内由拖入保存的新文件路径，用于对新加入的卡片播放落下动画
    recent_drop_paths: HashSet<PathBuf>,
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
            held_scale: None,
            drag_pending: None,
            view_offset: egui::Vec2::ZERO,
            view_scale: 1.0,
            settings_open: false,
            invert_pan: false,
            recent_drop_paths: HashSet::new(),
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
            let mut card = ImageCard::from_path(path.clone(), display_size);
            // 若为本次拖入保存的新图，播放落下动画：从上方落下并缩放收敛
            if self.recent_drop_paths.remove(&path) {
                card.anim_position_delta = Vec2::new(0.0, DROP_ANIM_INITIAL_OFFSET_Y);
                card.anim_scale = DROP_ANIM_INITIAL_SCALE;
            }
            self.cards.push(card);
            if self.cards.len() > MAX_CARDS {
                let removed = self.cards.remove(0);
                self.texture_cache.remove(&removed.path);
            }
        }
    }

    /// 处理本帧内拖入窗口的文件：已在监听目录的图只置顶不复制；新图复制/保存并标记落下动画。
    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        if dropped.is_empty() {
            return;
        }
        let watch_dir = std::path::PathBuf::from(&self.watch_path);
        for file in dropped {
            let saved = if let Some(ref path) = file.path {
                if !image_ops::is_image_path(path) {
                    None
                } else if image_ops::path_is_under(path, &watch_dir) {
                    // 已在监听目录：找到对应卡片并置顶，不复制
                    let dropped_canon = path.canonicalize().ok();
                    let idx = dropped_canon.and_then(|dc| {
                        self.cards.iter().position(|c| {
                            c.path.canonicalize().ok().as_ref() == Some(&dc)
                        })
                    });
                    if let Some(i) = idx {
                        self.bring_to_front(i);
                        ctx.request_repaint();
                    }
                    None
                } else {
                    image_ops::copy_image_to_dir(path, &watch_dir).ok()
                }
            } else if let Some(ref bytes) = file.bytes {
                let path = image_ops::unique_capture_path(&watch_dir);
                image_ops::save_image_from_bytes(bytes.as_ref(), &path).ok().map(|_| path)
            } else {
                None
            };
            if let Some(p) = saved {
                self.recent_drop_paths.insert(p);
                ctx.request_repaint();
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

    /// 一键打散：将所有图片随机均匀分布到当前可视区域内，带平滑位移动画。
    /// 面向 Mac/Windows 桌面端：用于快速浏览当前文件夹内有哪些图片。
    fn scatter_spread(&mut self) {
        use rand::seq::SliceRandom;
        use rand::Rng;
        let n = self.cards.len();
        if n == 0 {
            return;
        }
        let size = self.stack_layout.available_size;
        let pad = 48.0;
        let inner_w = (size.x - pad * 2.0).max(100.0);
        let inner_h = (size.y - pad * 2.0).max(100.0);
        let cols = (n as f32).sqrt().ceil().max(1.0) as usize;
        let rows = (n as f32 / cols as f32).ceil().max(1.0) as usize;
        let cell_w = inner_w / cols as f32;
        let cell_h = inner_h / rows as f32;
        let half_w = inner_w * 0.5;
        let half_h = inner_h * 0.5;
        let mut cells: Vec<egui::Vec2> = Vec::with_capacity(cols * rows);
        for r in 0..rows {
            for c in 0..cols {
                let cx = -half_w + (c as f32 + 0.5) * cell_w;
                let cy = -half_h + (r as f32 + 0.5) * cell_h;
                cells.push(egui::Vec2::new(cx, cy));
            }
        }
        let mut rng = rand::thread_rng();
        cells.shuffle(&mut rng);
        let jitter_scale = (cell_w.min(cell_h) * 0.28).min(24.0);
        for (i, card) in self.cards.iter_mut().enumerate() {
            let (layout_pos, _) = self.stack_layout.position_and_rotation(i, n);
            let layout_pos = layout_pos.to_vec2();
            let cell = cells.get(i).copied().unwrap_or(cells[0]);
            let jx = (rng.gen::<f32>() * 2.0 - 1.0) * jitter_scale;
            let jy = (rng.gen::<f32>() * 2.0 - 1.0) * jitter_scale;
            let target_world = cell + egui::Vec2::new(jx, jy);
            let new_offset = target_world - layout_pos;
            card.anim_position_delta = card.random_offset - new_offset;
            card.random_offset = new_offset;
            card.anim_rotation_offset = (rng.gen::<f32>() * 2.0 - 1.0) * 0.12;
        }
        self.view_scale = 1.0;
        self.view_offset = egui::Vec2::ZERO;
    }

    fn run_ui(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let dt = ctx.input(|i| i.stable_dt).min(0.1);

        // 关闭请求：直接关闭窗口
        if ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        self.poll_watch_events();

        // 快捷抓图：从浏览器或任意软件拖入窗口的图片，保存到监听目录并由 watcher 加入堆叠
        self.handle_dropped_files(ctx);

        // 底部栏：左下角「一览」按钮（桌面端：与标题栏/窗口控制区分离，减少误触）
        egui::TopBottomPanel::bottom("deckref_bottom_bar")
            .min_height(BOTTOM_BAR_HEIGHT)
            .max_height(BOTTOM_BAR_HEIGHT)
            .show(ctx, |ui| {
                ui.with_layout(
                    egui::Layout::left_to_right(egui::Align::Center).with_cross_justify(true),
                    |ui| {
                        if ui
                            .button(egui::RichText::new("一览").size(13.0))
                            .on_hover_text("将图片打散并均匀分布，快速浏览文件夹内容")
                            .clicked()
                        {
                            self.scatter_spread();
                        }
                    },
                );
            });

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
                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            if ui
                                .button(egui::RichText::new("⚙").size(16.0))
                                .on_hover_text("设置")
                                .clicked()
                            {
                                self.settings_open = true;
                            }
                        },
                    );
                });
                ui.add_space(4.0);

                let stack_rect = ui.available_rect_before_wrap();
                if stack_rect.is_positive() {
                    self.stack_layout.available_size = stack_rect.size();
                    let screen_center = stack_rect.center();
                    let n = self.cards.len();

                    // 中键拖拽平移视角（方向可由设置中的「反转平移方向」切换）
                    let middle_down = ui.input(|i| i.pointer.middle_down());
                    let pointer_delta = ui.input(|i| i.pointer.delta());
                    if middle_down && pointer_delta != egui::Vec2::ZERO {
                        let delta = if self.invert_pan { pointer_delta } else { -pointer_delta };
                        self.view_offset += delta;
                    }

                    let pointer = ui.input(|i| i.pointer.hover_pos());
                    let pointer_down = ui.input(|i| i.pointer.primary_down());
                    let pointer_released = ui.input(|i| i.pointer.primary_released());

                    let view_offset = self.view_offset;
                    let view_scale = self.view_scale;
                    let world_to_screen = |w: egui::Pos2| {
                        let v = w.to_vec2() - view_offset;
                        egui::Pos2::new(
                            screen_center.x + v.x * view_scale,
                            screen_center.y + v.y * view_scale,
                        )
                    };
                    let screen_to_world = |s: egui::Pos2| {
                        let v = egui::Vec2::new(
                            s.x - screen_center.x,
                            s.y - screen_center.y,
                        ) / view_scale;
                        egui::Pos2::new(view_offset.x + v.x, view_offset.y + v.y)
                    };

                    // 未拿起任何图片时：滚轮缩放整个视图（移近/移远），以指针位置为缩放中心
                    if self.picked_up.is_none() {
                        let scroll = ui.input(|i| i.raw_scroll_delta).y;
                        if scroll != 0.0 {
                            let world_before = pointer.map(|p| screen_to_world(p).to_vec2());
                            self.view_scale = (self.view_scale * (1.0 + scroll * VIEW_ZOOM_SENSITIVITY))
                                .clamp(VIEW_SCALE_MIN, VIEW_SCALE_MAX);
                            if let (Some(ptr), Some(wb)) = (pointer, world_before) {
                                let s_center = screen_center.to_vec2();
                                self.view_offset = wb - (ptr.to_vec2() - s_center) / self.view_scale;
                            }
                        }
                    }

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
                        let world_pos = world_pos + card.random_offset + card.anim_position_delta;
                        let rot = rot + card.random_rotation + card.anim_rotation_offset;
                        let size = self.stack_layout.effective_display_size(card.intrinsic_size, i, n);
                        let size = size * card.display_scale;
                        let half = size / 2.0;
                        if let Some(wp) = world_pointer {
                            if rotated_rect_contains(world_pos, half, rot, wp) {
                                hovered_index = Some(i);
                                break;
                            }
                        }
                    }

                    // 更新每张卡的 anim_scale：仅悬停时稍微放大，不移动位置
                    // 更新每张卡的 anim_rotation_offset：放下后向 0 收敛，实现旋转落定动画
                    // 更新每张卡的 anim_position_delta：打散等位移动画向零收敛
                    for (i, card) in self.cards.iter_mut().enumerate() {
                        let hovered = hovered_index == Some(i);
                        let target_scale = if hovered { 1.08 } else { 1.0 }; // 稍微放大
                        card.anim_scale = lerp_scale(card.anim_scale, target_scale, dt);
                        card.anim_rotation_offset = lerp_rotation(card.anim_rotation_offset, 0.0, dt);
                        card.anim_position_delta = lerp_position(card.anim_position_delta, egui::Vec2::ZERO, dt);
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
                        let world_pos = world_pos + card.random_offset + card.anim_position_delta;
                        let rot = rot + card.random_rotation + card.anim_rotation_offset;
                        let screen_pos = world_to_screen(world_pos);
                        let size = self.stack_layout.effective_display_size(card.intrinsic_size, i, n);
                        let size = size * card.display_scale * view_scale;
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
                            let rot = rot + card.random_rotation + card.anim_rotation_offset;
                            let size = self.stack_layout.effective_display_size(card.intrinsic_size, idx, n);
                            let held_scale = self.held_scale.unwrap_or(1.0);
                            let size = size * held_scale * view_scale;
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

                    // 长按超时 → 进入“拿起”状态（卡片将跟手，松手时放下并更新位置/顺序；移动时可拖出窗口）
                    if self.picked_up.is_none() && self.long_press.is_some() {
                        if let Some((idx, start)) = self.long_press {
                            if ctx.input(|i| i.time) - start >= LONG_PRESS_DURATION as f64 {
                                let (world_pos, _rot) = self.stack_layout.position_and_rotation(idx, n);
                                let card = &self.cards[idx];
                                let card_world = world_pos + card.random_offset + card.anim_position_delta;
                                let grab_offset_world = world_pointer
                                    .map(|wp| wp.to_vec2() - card_world.to_vec2())
                                    .unwrap_or(egui::Vec2::ZERO);
                                let now = ctx.input(|i| i.time);
                                self.picked_up = Some((idx, grab_offset_world, now));
                                self.held_smooth_pos = Some(card_world.to_vec2());
                                self.held_scale = Some(card.display_scale);
                                self.long_press = None;
                                self.drag_pending = Some(idx); // 拿起后指针离开窗口边缘时发起系统拖出
                            }
                        }
                    }

                    // 拿起时：滚轮缩放（30%～500%）
                    if let Some((_idx, _, _)) = self.picked_up {
                        let scroll = ui.input(|i| i.raw_scroll_delta).y;
                        if scroll != 0.0 {
                            let current = self.held_scale.get_or_insert(1.0);
                            *current = (*current * (1.0 + scroll * WHEEL_SCALE_SENSITIVITY))
                                .clamp(DISPLAY_SCALE_MIN, DISPLAY_SCALE_MAX);
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
                            let scale = self.held_scale.unwrap_or(1.0);
                            self.held_scale = None;
                            let drop_world = world_pointer
                                .map(|wp| wp.to_vec2() - grab_offset_world)
                                .unwrap_or(egui::Vec2::ZERO);
                            // 放下后卡片会置顶，故偏移应相对“最前一张”的布局位置；并记录旋转差用于落定动画
                            let (layout_front, new_layout_rot) = self.stack_layout.position_and_rotation(n.saturating_sub(1), n);
                            let (_, old_layout_rot) = self.stack_layout.position_and_rotation(idx, n);
                            let new_offset = drop_world - layout_front.to_vec2();
                            if let Some(card) = self.cards.get_mut(idx) {
                                card.random_offset = new_offset;
                                card.display_scale = scale;
                                card.anim_rotation_offset = normalize_angle_delta(old_layout_rot - new_layout_rot);
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

        // 设置窗口（右上角齿轮打开）
        egui::Window::new("设置")
            .open(&mut self.settings_open)
            .collapsible(false)
            .resizable(false)
            .default_width(260.0)
            .show(ctx, |ui| {
                ui.checkbox(
                    &mut self.invert_pan,
                    "反转平移方向（中键拖动）",
                )
                .on_hover_text("开启后，中键拖动时视角移动方向与默认相反");
            });

        // 长按拿起后指针离开窗口边缘：发起系统文件拖放，可拖到文件夹或其它软件
        if let Some(idx) = self.drag_pending {
            let screen_rect = ctx.screen_rect();
            let pointer = ctx.input(|i| i.pointer.hover_pos());
            // 指针在窗口外（或无法获取，如已移出）时转为系统拖放
            let pointer_outside = pointer
                .map(|p| !screen_rect.contains(p))
                .unwrap_or(true);
            if pointer_outside {
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
                self.picked_up = None;
                self.held_smooth_pos = None;
                self.held_scale = None;
            }
        }

        // 仅在需要连续重绘时请求 repaint，避免每帧强制重绘导致透明窗口抖动
        let needs_continuous_repaint = self.picked_up.is_some()
            || self.cards.iter().any(|c| {
                c.anim_position_delta.length_sq() > 1e-8
                    || c.anim_rotation_offset.abs() > 1e-6
                    || (c.anim_scale - 1.0).abs() > 1e-4
            });
        if needs_continuous_repaint {
            ctx.request_repaint();
        }
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
