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
/// 顶部地址栏高度（叠加在图片上层，带半透明底）
const TOP_BAR_HEIGHT: f32 = 40.0;
/// 视图缩放下限（移远），50%
const VIEW_SCALE_MIN: f32 = 0.5;
/// 视图缩放上限（移近），500%，便于阵列模式下滚轮放大查看单张
const VIEW_SCALE_MAX: f32 = 5.0;
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
    /// 刚放下并正在做旋转落定动画的卡片索引；仅该卡的 anim_rotation_offset 向 0 收敛，其余保持一览/阵列设定
    recently_dropped_index: Option<usize>,
    /// 本帧或上一帧内由拖入保存的新文件路径，用于对新加入的卡片播放落下动画
    recent_drop_paths: HashSet<PathBuf>,
    #[cfg(windows)]
    /// Windows 原生拖放（FileGroupDescriptorW + FileContents）收到的路径，每帧 drain 后统一按“拖入文件”处理
    win_drop_rx: Option<mpsc::Receiver<PathBuf>>,
    #[cfg(windows)]
    /// 保持 OLE 拖放目标注册，drop 时自动撤销
    _win_drop_guard: Option<crate::win_drop::DropTargetGuard>,
    #[cfg(windows)]
    /// 首帧再注册 OLE 拖放目标用的窗口句柄（避免被 winit 覆盖）
    win_drop_pending_hwnd: Option<isize>,
    /// 标题栏 logo 纹理（来自 assets/icon.png），无则只显示文字
    logo_texture: Option<TextureHandle>,
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
            recently_dropped_index: None,
            recent_drop_paths: HashSet::new(),
            #[cfg(windows)]
            win_drop_rx: None,
            #[cfg(windows)]
            _win_drop_guard: None,
            #[cfg(windows)]
            win_drop_pending_hwnd: None,
            logo_texture: None,
        };
        app.load_existing_images();
        Ok(app)
    }

    #[cfg(windows)]
    /// 设置 Windows 原生拖放通道与 guard（由 main 在窗口创建后调用）。
    pub fn set_win_drop(&mut self, rx: mpsc::Receiver<PathBuf>, guard: crate::win_drop::DropTargetGuard) {
        self.win_drop_rx = Some(rx);
        self._win_drop_guard = Some(guard);
    }

    #[cfg(windows)]
    /// 设置待注册的窗口句柄，首帧 run_ui 时再注册 OLE 拖放（避免被 winit 覆盖）。
    pub fn set_win_drop_pending_hwnd(&mut self, hwnd: isize) {
        self.win_drop_pending_hwnd = Some(hwnd);
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

    /// 处理单条“拖入路径”（来自 Windows 原生拖放或 egui 的 path）：已在监听目录则置顶，否则复制并标记落下动画。
    fn handle_dropped_path(&mut self, path: PathBuf, ctx: &egui::Context) {
        let watch_dir = std::path::PathBuf::from(&self.watch_path);
        if !image_ops::is_image_path(&path) {
            return;
        }
        if image_ops::path_is_under(&path, &watch_dir) {
            let dropped_canon = path.canonicalize().ok();
            let idx = dropped_canon.as_ref().and_then(|dc| {
                self.cards
                    .iter()
                    .position(|c| c.path.canonicalize().ok().as_ref() == Some(dc))
            });
            if let Some(i) = idx {
                self.bring_to_front(i);
                ctx.request_repaint();
            } else if let Some(canon) = dropped_canon {
                // 路径已在监听目录但尚无卡片（如刚由 FileContents 写入），标记以便 watcher 添加时播落下动画
                self.recent_drop_paths.insert(canon);
                ctx.request_repaint();
            }
            return;
        }
        if let Ok(p) = image_ops::copy_image_to_dir(&path, &watch_dir) {
            self.recent_drop_paths.insert(p);
            ctx.request_repaint();
        }
    }

    /// 从剪贴板读取图片并保存到监听目录（用于 Chrome 等拖入无效时：复制图片后 Ctrl+V 粘贴）。
    fn try_paste_from_clipboard(&mut self, ctx: &egui::Context) {
        let watch_dir = std::path::PathBuf::from(&self.watch_path);
        let mut clipboard = match arboard::Clipboard::new() {
            Ok(c) => c,
            Err(_) => return,
        };
        let img = match clipboard.get_image() {
            Ok(d) => d,
            Err(_) => return,
        };
        let path = image_ops::unique_capture_path(&watch_dir);
        let bytes: Vec<u8> = img.bytes.to_vec();
        let w = img.width.try_into().unwrap_or(0u32);
        let h = img.height.try_into().unwrap_or(0u32);
        if w > 0 && h > 0 && image_ops::save_rgba_as_png(w, h, &bytes, &path).is_ok() {
            self.recent_drop_paths.insert(path);
            ctx.request_repaint();
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

    /// 若存在 assets/icon.png 则加载为标题栏 logo 纹理，只尝试一次，失败不重试。
    fn ensure_logo_texture(&mut self, ctx: &egui::Context) {
        if self.logo_texture.is_some() {
            return;
        }
        let path = match image_ops::resolve_assets_icon_path() {
            Some(p) => p,
            None => return,
        };
        let img = match image_ops::load_image(&path) {
            Ok(i) => i,
            Err(_) => return,
        };
        let size = [img.width() as usize, img.height() as usize];
        let rgba = img.to_rgba8();
        let color_image = ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
        let handle = ctx.load_texture("deckref_logo", color_image, Default::default());
        self.logo_texture = Some(handle);
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
            card.display_scale = 1.0;
        }
        self.view_scale = 1.0;
        self.view_offset = egui::Vec2::ZERO;
    }

    /// 阵列排列：将图片按网格、原比例（contain）排列，单次触发；之后仍可自由拖动、拿起等。
    fn grid_arrange(&mut self) {
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
        for (i, card) in self.cards.iter_mut().enumerate() {
            let r = i / cols;
            let c = i % cols;
            let grid_center = egui::Vec2::new(
                -half_w + (c as f32 + 0.5) * cell_w,
                -half_h + (r as f32 + 0.5) * cell_h,
            );
            let (layout_pos, layout_rot) = self.stack_layout.position_and_rotation(i, n);
            let layout_pos = layout_pos.to_vec2();
            let new_offset = grid_center - layout_pos;
            card.anim_position_delta = card.random_offset - new_offset;
            card.random_offset = new_offset;
            card.anim_rotation_offset = 0.0;
            card.random_rotation = -layout_rot;
            let iw = card.intrinsic_size.x.max(1.0);
            let ih = card.intrinsic_size.y.max(1.0);
            let contain_scale = (cell_w / iw).min(cell_h / ih);
            let contain_size = egui::Vec2::new(iw * contain_scale, ih * contain_scale);
            let effective = self.stack_layout.effective_display_size(card.intrinsic_size, i, n);
            let eff_x = effective.x.max(1e-6);
            let eff_y = effective.y.max(1e-6);
            let display_scale = (contain_size.x / eff_x).min(contain_size.y / eff_y);
            card.display_scale = display_scale.clamp(DISPLAY_SCALE_MIN, DISPLAY_SCALE_MAX);
        }
        self.view_scale = 1.8;
        self.view_offset = egui::Vec2::ZERO;
    }

    fn run_ui(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let dt = ctx.input(|i| i.stable_dt).min(0.1);

        // Windows：首帧再注册 OLE 拖放目标，避免被 winit 覆盖导致 Chrome 拖入显示禁止
        #[cfg(windows)]
        if let Some(hwnd) = self.win_drop_pending_hwnd.take() {
            let (tx, rx) = mpsc::channel();
            let watch_path = std::path::PathBuf::from(&self.watch_path);
            match crate::win_drop::register_drop_target(hwnd, tx, watch_path) {
                Ok(guard) => {
                    self.win_drop_rx = Some(rx);
                    self._win_drop_guard = Some(guard);
                }
                Err(e) => {
                    eprintln!("[DeckRef] RegisterDragDrop failed: {}", e);
                }
            }
        }

        // 关闭请求：直接关闭窗口
        if ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        self.poll_watch_events();

        // 快捷抓图：从浏览器或任意软件拖入窗口的图片，保存到监听目录并由 watcher 加入堆叠
        self.handle_dropped_files(ctx);

        #[cfg(windows)]
        if let Some(rx) = &mut self.win_drop_rx {
            let paths: std::vec::Vec<PathBuf> = rx.try_iter().collect();
            for path in paths {
                self.handle_dropped_path(path, ctx);
            }
        }

        // 剪贴板粘贴：Ctrl+V / Cmd+V 将剪贴板中的图片保存到监听目录（Chrome 拖入无效时的备用方案）
        if ctx.input(|i| {
            (i.modifiers.ctrl || i.modifiers.mac_cmd) && i.key_pressed(egui::Key::V)
        }) {
            ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::V));
            ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::V));
            self.try_paste_from_clipboard(ctx);
        }

        // 底部栏改为叠加层，与地址栏一起在图片上层（先不占位，由后面的 CentralPanel 占满）
        // 堆叠区域（仅卡片；地址栏与底部栏在顶层 Area 叠加，不被图片遮挡）
        egui::CentralPanel::default()
            .frame(egui::Frame::none())
            .show(ctx, |ui| {
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
                    // 仅对“刚放下”的那张卡做 anim_rotation_offset 向 0 收敛（落定动画）；其余卡片保持一览/阵列设定的随机旋转
                    // 更新每张卡的 anim_position_delta：打散等位移动画向零收敛
                    let dropped_idx = self.recently_dropped_index;
                    for (i, card) in self.cards.iter_mut().enumerate() {
                        let hovered = hovered_index == Some(i);
                        let target_scale = if hovered { 1.08 } else { 1.0 }; // 稍微放大
                        card.anim_scale = lerp_scale(card.anim_scale, target_scale, dt);
                        if dropped_idx == Some(i) {
                            card.anim_rotation_offset = lerp_rotation(card.anim_rotation_offset, 0.0, dt);
                        }
                        card.anim_position_delta = lerp_position(card.anim_position_delta, egui::Vec2::ZERO, dt);
                    }
                    if dropped_idx.is_some()
                        && self.cards[dropped_idx.unwrap()].anim_rotation_offset.abs() < 1e-5
                    {
                        self.recently_dropped_index = None;
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
                            // 放下时：用当前视觉总旋转作为落定起点，目标为竖直，避免阵列下出现大角度旋转
                            let current_total_rot = if let Some(card) = self.cards.get(idx) {
                                old_layout_rot + card.random_rotation + card.anim_rotation_offset
                            } else {
                                old_layout_rot - new_layout_rot
                            };
                            // 在 bring_to_front 前记录所有卡片的当前世界坐标与总旋转角，以便重排后恢复其余卡片位置和朝向
                            let old_world_pos: Vec<egui::Vec2> = (0..n)
                                .map(|i| {
                                    let (lp, _) = self.stack_layout.position_and_rotation(i, n);
                                    lp.to_vec2() + self.cards[i].random_offset + self.cards[i].anim_position_delta
                                })
                                .collect();
                            let old_layout_rots: Vec<f32> = (0..n)
                                .map(|i| self.stack_layout.position_and_rotation(i, n).1)
                                .collect();
                            let old_total_rots: Vec<f32> = (0..n)
                                .map(|i| old_layout_rots[i] + self.cards[i].random_rotation + self.cards[i].anim_rotation_offset)
                                .collect();
                            if let Some(card) = self.cards.get_mut(idx) {
                                card.random_offset = new_offset;
                                card.display_scale = scale;
                                card.random_rotation = -new_layout_rot;
                                card.anim_rotation_offset = normalize_angle_delta(current_total_rot);
                            }
                            self.bring_to_front(idx);
                            // 重排后：被移动的卡片现在在 n-1，其余前移的卡片需补偿位置和旋转，使视觉不变
                            for i in idx..(n - 1) {
                                let (new_lp, new_layout_rot) = self.stack_layout.position_and_rotation(i, n);
                                let new_lp = new_lp.to_vec2();
                                self.cards[i].random_offset = old_world_pos[i + 1] - new_lp;
                                self.cards[i].anim_position_delta = egui::Vec2::ZERO;
                                // 保持总旋转角不变，否则 layout_rot 随索引变化会导致未移动的卡“看起来在转”
                                self.cards[i].anim_rotation_offset = normalize_angle_delta(
                                    old_total_rots[i + 1] - new_layout_rot - self.cards[i].random_rotation,
                                );
                            }
                            self.recently_dropped_index = Some(n - 1);
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

        // 顶层地址栏：叠加在图片之上，带半透明底，确保不被图片遮挡
        let screen = ctx.screen_rect();
        let toolbar_rect = Rect::from_min_max(
            screen.min,
            egui::Pos2::new(screen.max.x, screen.min.y + TOP_BAR_HEIGHT),
        );
        egui::Area::new(egui::Id::new("deckref_toolbar"))
            .order(egui::Order::Foreground)
            .fixed_pos(toolbar_rect.min)
            .constrain(true)
            .show(ctx, |ui| {
                ui.set_min_size(egui::Vec2::new(screen.width(), TOP_BAR_HEIGHT));
                // 半透明底，增加质感
                let fill = ui.visuals().window_fill();
                let c = egui::Color32::from(fill);
                let a = (c.a() as f32 * 0.92).round() as u8;
                let bg = egui::Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a);
                ui.painter().rect_filled(
                    Rect::from_min_size(egui::Pos2::ZERO, ui.available_size()),
                    egui::Rounding::same(0.0),
                    bg,
                );
                self.ensure_logo_texture(ctx);
                ui.horizontal(|ui| {
                    if let Some(ref tex) = self.logo_texture {
                        let size = 24.0;
                        ui.add(
                            egui::Image::new(egui::load::SizedTexture::new(
                                tex.id(),
                                egui::Vec2::new(size, size),
                            ))
                            .max_width(size)
                            .max_height(size),
                        );
                        ui.add_space(4.0);
                    }
                    ui.label("DeckRef");
                    ui.separator();
                    ui.label(format!("监听: {}", self.watch_path))
                        .on_hover_text("从 Chrome 拖入无效时，可复制图片后 Ctrl+V（Mac：Cmd+V）粘贴保存");
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
            });

        // 底层底部栏：叠加在图片之上，带半透明底
        let screen = ctx.screen_rect();
        let bottom_rect = Rect::from_min_max(
            egui::Pos2::new(screen.min.x, screen.max.y - BOTTOM_BAR_HEIGHT),
            screen.max,
        );
        egui::Area::new(egui::Id::new("deckref_bottom_bar"))
            .order(egui::Order::Foreground)
            .fixed_pos(bottom_rect.min)
            .constrain(true)
            .show(ctx, |ui| {
                ui.set_min_size(egui::Vec2::new(screen.width(), BOTTOM_BAR_HEIGHT));
                let fill = ui.visuals().window_fill();
                let c = egui::Color32::from(fill);
                let a = (c.a() as f32 * 0.92).round() as u8;
                let bg = egui::Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a);
                ui.painter().rect_filled(
                    Rect::from_min_size(egui::Pos2::ZERO, ui.available_size()),
                    egui::Rounding::same(0.0),
                    bg,
                );
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
                        if ui
                            .button(egui::RichText::new("阵列").size(13.0))
                            .on_hover_text("按网格、原比例排列，快速观看全貌；之后仍可自由拖动")
                            .clicked()
                        {
                            self.grid_arrange();
                        }
                    },
                );
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

        // 仅在需要连续重绘时请求 repaint，避免每帧强制重绘导致透明窗口抖动。
        // 包含缩放/平移：一览与阵列都会重置 view，之后用户仍可滚轮缩放、中键平移，必须持续重绘才能生效。
        let needs_continuous_repaint = self.picked_up.is_some()
            || (self.view_scale - 1.0).abs() > 1e-6
            || self.view_offset.length_sq() > 1e-6
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
