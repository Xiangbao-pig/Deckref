//! UI 逻辑：堆叠照片、纹理缓存、悬停/长按/排序。

use crate::app_nav::THIS_PC_SENTINEL;
use crate::error::AppResult;
use crate::image_card::{
    CardSortOrder, ImageCard, StackLayout, DISPLAY_SCALE_MAX, DISPLAY_SCALE_MIN,
    DROP_ANIM_INITIAL_OFFSET_Y, DROP_ANIM_INITIAL_SCALE,
};
use crate::image_ops;
use crate::mode::AppMode;
use crate::settings;
use crate::watcher::{list_existing_image_paths, WatchEvent};
use crate::window_state;
use crate::app_threads::RepaintHook;
use crate::app_viewport::CardScreenRect;
use crate::layout;
use eframe::egui::{self, ColorImage, TextureHandle, Vec2};
use rstar::RTree;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::SystemTime;

mod stack_cull_draw;
mod stack_interaction;
mod stack_viewport;
mod ui_bottom_bar;
mod ui_mode_sidebar;
mod ui_stack_canvas;
mod ui_top_bar;

const MAX_CARDS: usize = 80;
/// 灵感模式下「最近图片」上限（Windows 最近使用的文件）
const INSPIRATION_RECENT_MAX: usize = 30;
const LONG_PRESS_DURATION: f32 = 0.5;
/// 视图缩放下限（移远），50%
const VIEW_SCALE_MIN: f32 = 0.5;
/// 视图缩放上限（移近），500%，便于阵列模式下滚轮放大查看单张
const VIEW_SCALE_MAX: f32 = 5.0;
/// 画板模式无布局时使用的固定画布参考尺寸，与窗口解耦（与 StackLayout 默认一致）
const BOARD_DEFAULT_CANVAS_SIZE: egui::Vec2 = egui::Vec2 { x: 600.0, y: 400.0 };
/// 未拿起图片时滚轮调节视图缩放的灵敏度
const VIEW_ZOOM_SENSITIVITY: f32 = 0.0012;
/// 记录「刚拖出」路径的上限，避免集合无限增长
const MAX_DRAG_OUT_RECENT: usize = 20;
/// 导航栏「最近到访」下拉中保留的文件夹数量，并为这 5 个做卡片缓存以加快切换
const RECENT_FOLDERS_MAX: usize = 5;
/// 纹理缓存最多保留的文件夹数量；超过后淘汰最久未访问的文件夹下全部纹理，控制显存与内存占用。
const MAX_TEXTURE_CACHE_FOLDERS: usize = 10;
/// 右键拖动移动窗口：超过该距离（像素）才视为「移动窗口」而非右键点击，与 PureRef 的 Move window threshold 思路一致
const RIGHT_DRAG_MOVE_THRESHOLD: f32 = 4.0;
/// 剪贴板截图轮询间隔（秒）：至少隔这么久才读一次剪贴板，避免过于频繁。1.0～1.5 更省资源，0.8 响应更快。
const CLIPBOARD_POLL_INTERVAL: f64 = 0.8;
/// 按下 Shift 后持续监听剪贴板截图的时长（秒）。手动截屏（如 Win+Shift+S）通常伴随 Shift，此时间内剪贴板新图会保存；超时后不再自动保存，避免误存 QQ/浏览器等复制的图片。
const SHIFT_LISTENING_DURATION: f64 = 5.0;

/// 拿起状态：卡片索引 + 抓取时指针相对卡片中心的世界坐标偏移（用于跟手）
/// 以及拿起时刻的时间（用于拿起动画插值）
type PickedUpState = (usize, egui::Vec2, f64);

/// 应用状态：监听线程 + 卡片堆叠 + 纹理缓存 + 视角平移。
pub struct DeckRefApp {
    watch_rx: mpsc::Receiver<WatchEvent>,
    _watcher_handle: Option<thread::JoinHandle<AppResult<()>>>,
    cards: Vec<ImageCard>,
    /// 切换模式时保留另一模式的卡片列表，再次切回时直接恢复，避免重扫磁盘与卡顿
    cached_default_cards: Option<Vec<ImageCard>>,
    cached_inspiration_cards: Option<Vec<ImageCard>>,
    cached_board_cards: Option<Vec<ImageCard>>,
    texture_cache: std::collections::HashMap<PathBuf, TextureHandle>,
    /// 纹理缓存对应的文件夹路径（按最近使用顺序，前新后旧）；超过 MAX_TEXTURE_CACHE_FOLDERS 时淘汰最旧文件夹的全部纹理。
    texture_cache_scope: Vec<PathBuf>,
    /// 应用根目录（如 Pictures/Ref），各模式的 watch_path 由此派生
    base_path: PathBuf,
    /// 截图目录（Ref/Screenshots），运行时剪贴板截图自动保存到此，灵感模式会合并展示
    screenshot_dir: PathBuf,
    /// 当前模式；切换时重建 watcher 并重载卡片
    mode: AppMode,
    /// 当前监听路径（= mode.watch_path(&base_path)），与 watcher 一致
    watch_path: String,
    /// 默认模式下的前进/返回历史；索引指向当前显示的路径
    nav_history: Vec<PathBuf>,
    nav_index: usize,
    /// 地址栏是否处于「输入/粘贴路径」编辑模式
    address_bar_editing: bool,
    /// 编辑模式下的路径字符串（可输入或粘贴）
    address_bar_edit_text: String,
    /// 进入编辑模式后下一帧请求焦点
    address_bar_edit_request_focus: bool,
    /// 最近到访的文件夹路径（最多 5 个，最新在前），用于导航栏右侧下拉
    recent_folders: Vec<PathBuf>,
    /// 最近到访文件夹的卡片缓存，切换回时直接恢复、无需重新加载
    recent_folder_cards: std::collections::HashMap<PathBuf, Vec<ImageCard>>,
    stack_layout: StackLayout,
    /// 当前内容区尺寸（每帧更新），仅在执行 阵列/一览 时写入 stack_layout.available_size，避免拖边调整窗口时已排好的图移位
    last_stack_size: egui::Vec2,
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
    /// true = 阵列排列，false = 一览打散；默认 true，启动时首帧应用阵列
    grid_view: bool,
    /// 当前卡片列表是否已做过首次排列（切换目录/模式后重置，首帧再应用 grid_view）
    initial_arrange_done: bool,
    /// 设置面板是否打开
    settings_open: bool,
    /// 反转中键平移方向（为 true 时与默认相反，用于适应不同习惯）
    invert_pan: bool,
    /// 拿起图片后拖出窗口边缘时是否发起系统文件拖放（可在设置中关闭，默认开启）
    allow_drag_out: bool,
    /// 预览专业格式图片（TGA、ICO、DDS、AVIF、EXR、HDR、PNM、QOI、PSD 等）；开启后监听与加载时包含这些扩展名
    preview_professional_formats: bool,
    /// 框选方式：开 = AutoDesk 式（左→右 窗口选择，右→左 交叉选择）；关 = 统一交叉选择
    autodesk_style_selection: bool,
    /// 框选多选：当前选中的卡片索引（画板模式成组等 Phase 2 后续使用）
    selected_indices: HashSet<usize>,
    /// 框选进行中：空白处按下时的屏幕坐标，松手时根据选框与设置计算选中集
    marquee_start_screen: Option<egui::Pos2>,
    /// 框选进行中：当前拖拽端点的屏幕坐标；鼠标移出窗口时保留上一帧值并夹在窗口边（参考 Windows 资源管理器，选框顶住边不消失）
    marquee_last_end_screen: Option<egui::Pos2>,
    /// 刚放下并正在做旋转落定动画的卡片索引；仅该卡的 anim_rotation_offset 向 0 收敛，其余保持一览/阵列设定
    recently_dropped_index: Option<usize>,
    /// 本帧或上一帧内由拖入保存的新文件路径，用于对新加入的卡片播放落下动画
    recent_drop_paths: HashSet<PathBuf>,
    /// 最近从窗口拖出的图片的规范路径；若用户未真正保存到别处又把同一张图拖回，则只置顶不复制，防止循环保存
    recent_drag_out_paths: HashSet<PathBuf>,
    #[cfg(windows)]
    /// Windows 原生拖放（FileGroupDescriptorW + FileContents）收到的路径，每帧 drain 后统一按“拖入文件”处理
    win_drop_rx: Option<mpsc::Receiver<PathBuf>>,
    #[cfg(windows)]
    /// 保持 OLE 拖放目标注册，drop 时自动撤销
    _win_drop_guard: Option<crate::win_drop::DropTargetGuard>,
    #[cfg(windows)]
    /// 首帧再注册 OLE 拖放目标用的窗口句柄（避免被 winit 覆盖）；模式切换时复用此句柄重新注册
    win_drop_pending_hwnd: Option<isize>,
    #[cfg(windows)]
    /// 已注册的窗口句柄，模式切换时用于重新注册到新 watch_path
    win_drop_hwnd: Option<isize>,
    /// 标题栏 logo 纹理（来自 assets/icon.png），当前未使用，保留供后续使用）
    #[allow(dead_code)]
    logo_texture: Option<TextureHandle>,
    /// 启动/切换模式时异步加载卡片：后台线程发送 (path, dimensions, metadata)，主线程每帧消费
    loading_rx: Option<mpsc::Receiver<(PathBuf, (u32, u32), (u64, SystemTime))>>,
    /// 异步加载线程的 JoinHandle，channel 关闭后 join 回收
    loading_handle: Option<thread::JoinHandle<()>>,
    /// 上次检测到的剪贴板图片哈希，用于避免重复保存同一张截图
    last_clipboard_image_hash: Option<u64>,
    /// 上次轮询剪贴板的时间（秒），用于节流
    last_clipboard_check_time: f64,
    /// 仅当「Shift 按下后的 N 秒内」才把剪贴板新图当作截图保存；超过此刻则不保存（避免误存 QQ/浏览器等复制的图）。0 表示未在监听窗口内。
    shift_listening_until: f64,
    /// 设置中更改「预览专业格式」后置为 true，本帧末重启 watcher 并重新加载
    pending_reload_after_settings: bool,
    /// 右键按下后尚未超过移动阈值的累计位移（用于阈值判断）
    right_drag_pending_delta: egui::Vec2,
    /// 本手势已超过阈值，正在移动窗口（Windows 上 StartDrag 仅响应左键，故用手动 OuterPosition）
    right_drag_started: bool,
    /// 上一帧发送的窗口位置（目标 = last_sent + delta 取整，减少累积误差）
    right_drag_last_sent: Option<egui::Pos2>,
    /// 纹理加载请求发送端：主线程发送 path 和目标最长边，后台线程解码后通过 texture_loading_rx 回传 RGBA
    texture_loading_tx: Option<mpsc::Sender<(PathBuf, u32)>>,
    /// 后台解码完成的纹理 (path, rgba, width, height)，主线程每帧取若干做 GPU 上传
    texture_loading_rx: Option<mpsc::Receiver<(PathBuf, Vec<u8>, u32, u32)>>,
    /// 纹理加载线程 handle，进程退出时 drop 即可
    _texture_loading_handle: Option<thread::JoinHandle<()>>,
    /// 画板模式：上次成功写入布局的时间（秒），用于防抖保存
    last_board_layout_save_time: f64,
    /// 画板模式：加载的布局若带 reference_available_size，首帧用当前画布尺寸按比例缩放位置与缩放后清除
    layout_reference_size: Option<egui::Vec2>,
    /// 画板模式专用视口：缩放倍率（1.0=100%），仅画板模式读写，与默认/灵感完全解耦
    board_view_scale: f32,
    /// 画板模式专用视口：平移偏移（世界坐标），仅画板模式读写
    board_view_offset: egui::Vec2,
    /// 画板模式：上一帧内容区中心（屏幕坐标），用于窗口 resize 时补偿 offset 使画布中心不跳动
    last_board_screen_center: Option<egui::Vec2>,
    /// 默认/灵感模式：上一帧堆叠区中心（屏幕坐标），用于窗口 resize 时补偿 view_offset，避免所有图片移位
    last_stack_screen_center: Option<egui::Vec2>,
    /// 已请求但尚未收到的纹理目标边长。同一张图只保留当前最大的请求，避免总览档和图放大档互相覆盖。
    pending_texture_paths: HashMap<PathBuf, u32>,
    /// 后台线程用来在异步结果到达时唤醒 UI。
    repaint_ctx: RepaintHook,
    /// 监听事件的 dimensions/metadata 请求。generation 用于丢弃切换文件夹前的旧任务。
    watch_io_tx: Option<mpsc::Sender<(u64, PathBuf)>>,
    watch_io_rx: Option<mpsc::Receiver<crate::app_threads::WatchIoDone>>,
    _watch_io_handle: Option<thread::JoinHandle<()>>,
    watch_generation: Arc<AtomicU64>,
    /// 已提交、结果尚未回到主线程的监听 IO 数。切换文件夹时清零。
    watch_io_inflight: usize,
    /// 上一帧屏幕 AABB 的 R-Tree；布局未变时复用，避免每帧 bulk_load。
    screen_rtree: Option<RTree<CardScreenRect>>,
    /// 与 `screen_rtree` 对应的布局指纹。
    screen_rtree_key: Option<u64>,
    /// 上一帧命中检测使用的屏幕指针；未移动且布局未变时可跳过精确测试。
    last_hover_screen: Option<egui::Pos2>,
    last_hovered_index: Option<usize>,
    last_hover_key: Option<u64>,
}

impl DeckRefApp {
    /// 使用应用根目录（如 Pictures/Ref）创建应用；默认使用「默认」模式，灵感模式目录会在首次切换时创建。
    pub fn new(base_path: PathBuf) -> AppResult<Self> {
        let mode = AppMode::Default;
        let _ = mode.ensure_dir(&base_path);
        let _ = AppMode::Inspiration.ensure_dir(&base_path); // 提前创建，便于用户直接往该目录放图
        let screenshot_dir = base_path.join("Screenshots");
        let _ = std::fs::create_dir_all(&screenshot_dir);
        let default_watch = mode.watch_path(&base_path);
        let watch_path = window_state::load_last_default_path()
            .filter(|p| p.is_dir())
            .unwrap_or(default_watch);
        let loaded = settings::load_settings();
        let repaint_ctx: RepaintHook = Arc::new(Mutex::new(None));
        let watch_generation = Arc::new(AtomicU64::new(0));
        let (rx, handle) = crate::app_threads::spawn_waking_watcher(
            watch_path.clone(),
            loaded.preview_professional_formats,
            repaint_ctx.clone(),
        )?;
        let (tex_req_tx, tex_req_rx) = mpsc::channel();
        let (tex_res_tx, tex_res_rx) = mpsc::channel();
        let tex_repaint = repaint_ctx.clone();
        let tex_handle = thread::Builder::new()
            .name("deckref-tex-loader".into())
            .spawn(move || {
                crate::app_threads::run_texture_loader_thread(tex_req_rx, tex_res_tx, tex_repaint)
            });
        let (texture_loading_tx, texture_loading_rx, _texture_loading_handle) = match tex_handle {
            Ok(h) => (Some(tex_req_tx), Some(tex_res_rx), Some(h)),
            Err(_) => (None, None, None),
        };
        let (watch_io_tx_thread, watch_io_rx_thread) = mpsc::channel();
        let (watch_io_res_tx, watch_io_res_rx) = mpsc::channel();
        let watch_io_generation = watch_generation.clone();
        let watch_io_repaint = repaint_ctx.clone();
        let watch_io_handle = thread::Builder::new()
            .name("deckref-watch-io".into())
            .spawn(move || {
                crate::app_threads::run_watch_io_thread(
                    watch_io_rx_thread,
                    watch_io_res_tx,
                    watch_io_generation,
                    watch_io_repaint,
                )
            });
        let (watch_io_tx, watch_io_rx, _watch_io_handle) = match watch_io_handle {
            Ok(h) => (Some(watch_io_tx_thread), Some(watch_io_res_rx), Some(h)),
            Err(_) => (None, None, None),
        };
        let mut app = Self {
            watch_rx: rx,
            _watcher_handle: Some(handle),
            cards: Vec::new(),
            cached_default_cards: None,
            cached_inspiration_cards: None,
            cached_board_cards: None,
            texture_cache: std::collections::HashMap::new(),
            texture_cache_scope: Vec::new(),
            base_path,
            screenshot_dir,
            mode,
            watch_path: watch_path.display().to_string(),
            nav_history: vec![watch_path.clone()],
            nav_index: 0,
            address_bar_editing: false,
            address_bar_edit_text: String::new(),
            address_bar_edit_request_focus: false,
            recent_folders: {
                let loaded = window_state::load_recent_folders(RECENT_FOLDERS_MAX);
                let mut rf = vec![watch_path.clone()];
                for p in loaded {
                    if p != watch_path && !rf.contains(&p) {
                        rf.push(p);
                        if rf.len() >= RECENT_FOLDERS_MAX {
                            break;
                        }
                    }
                }
                rf
            },
            recent_folder_cards: std::collections::HashMap::new(),
            stack_layout: StackLayout::default(),
            last_stack_size: egui::Vec2::new(600.0, 400.0),
            sort_order: CardSortOrder::default(),
            long_press: None,
            picked_up: None,
            held_smooth_pos: None,
            held_scale: None,
            drag_pending: None,
            view_offset: egui::Vec2::ZERO,
            view_scale: 1.0,
            grid_view: true,
            initial_arrange_done: false,
            settings_open: false,
            invert_pan: loaded.invert_pan,
            allow_drag_out: loaded.allow_drag_out,
            preview_professional_formats: loaded.preview_professional_formats,
            autodesk_style_selection: loaded.autodesk_style_selection,
            selected_indices: HashSet::new(),
            marquee_start_screen: None,
            marquee_last_end_screen: None,
            recently_dropped_index: None,
            recent_drop_paths: HashSet::new(),
            recent_drag_out_paths: HashSet::new(),
            #[cfg(windows)]
            win_drop_rx: None,
            #[cfg(windows)]
            _win_drop_guard: None,
            #[cfg(windows)]
            win_drop_pending_hwnd: None,
            #[cfg(windows)]
            win_drop_hwnd: None,
            logo_texture: None,
            loading_rx: None,
            loading_handle: None,
            last_clipboard_image_hash: None,
            last_clipboard_check_time: 0.0,
            shift_listening_until: 0.0,
            pending_reload_after_settings: false,
            right_drag_pending_delta: egui::Vec2::ZERO,
            right_drag_started: false,
            right_drag_last_sent: None,
            texture_loading_tx,
            texture_loading_rx,
            _texture_loading_handle,
            last_board_layout_save_time: 0.0,
            layout_reference_size: None,
            board_view_scale: 1.0,
            board_view_offset: egui::Vec2::ZERO,
            last_board_screen_center: None,
            last_stack_screen_center: None,
            pending_texture_paths: HashMap::new(),
            repaint_ctx,
            watch_io_tx,
            watch_io_rx,
            _watch_io_handle,
            watch_generation,
            watch_io_inflight: 0,
            screen_rtree: None,
            screen_rtree_key: None,
            last_hover_screen: None,
            last_hovered_index: None,
            last_hover_key: None,
        };
        app.start_async_load();
        Ok(app)
    }

    /// 切换到指定模式：重建 watcher；若有该模式的缓存则直接恢复卡片（避免重扫磁盘与卡顿），否则从磁盘加载。纹理缓存保留，切回时无需重新解码。
    fn switch_mode(&mut self, new_mode: AppMode) {
        if self.mode == new_mode {
            return;
        }
        // 离开画板前保存当前画板布局与路径，并同步到默认/画板两处（易用性：两模式文件夹同步）
        if self.mode == AppMode::Board && !self.cards.is_empty() {
            let path = PathBuf::from(&self.watch_path);
            let canvas_size = self.layout_reference_size.unwrap_or(self.last_stack_size);
            let _ = layout::save_layout(&path, &self.cards, canvas_size);
            let _ = window_state::save_last_folder_sync(&path);
        }
        // 将当前模式的卡片列表存入对应缓存
        let old_mode = self.mode;
        match old_mode {
            AppMode::Default => self.cached_default_cards = Some(std::mem::take(&mut self.cards)),
            AppMode::Inspiration => self.cached_inspiration_cards = Some(std::mem::take(&mut self.cards)),
            AppMode::Board => self.cached_board_cards = Some(std::mem::take(&mut self.cards)),
        }

        let _ = new_mode.ensure_dir(&self.base_path);
        let watch_path = if new_mode == AppMode::Board {
            window_state::load_last_board_path()
                .filter(|p| p.is_dir())
                .unwrap_or_else(|| new_mode.watch_path(&self.base_path))
        } else if new_mode == AppMode::Default {
            window_state::load_last_default_path()
                .filter(|p| p.is_dir())
                .unwrap_or_else(|| new_mode.watch_path(&self.base_path))
        } else {
            new_mode.watch_path(&self.base_path)
        };
        if let Ok((rx, handle)) = crate::app_threads::spawn_waking_watcher(
            watch_path.clone(),
            self.preview_professional_formats,
            self.repaint_ctx.clone(),
        ) {
            self._watcher_handle = Some(handle);
            self.watch_rx = rx;
        }
        self.watch_generation.fetch_add(1, Ordering::Relaxed);
        self.watch_io_inflight = 0;
        self.watch_path = watch_path.display().to_string();
        self.mode = new_mode;
        if new_mode == AppMode::Default || new_mode == AppMode::Board {
            self.nav_history = vec![watch_path.clone()];
            self.nav_index = 0;
            self.push_recent_folder(watch_path.clone());
            if watch_path.is_dir() {
                let _ = window_state::save_last_folder_sync(&watch_path);
            }
        }
        self.address_bar_editing = false;
        self.long_press = None;
        self.picked_up = None;
        self.held_smooth_pos = None;
        self.held_scale = None;
        self.drag_pending = None;
        self.selected_indices.clear();
        self.marquee_start_screen = None;
        self.marquee_last_end_screen = None;
        self.recent_drop_paths.clear();
        // 不清空 texture_cache，切回时已解码的纹理可直接复用，避免卡顿
        self.recent_drag_out_paths.clear();
        if new_mode == AppMode::Board {
            self.last_board_screen_center = None;
            self.last_stack_screen_center = None;
        }

        // 从缓存恢复或从磁盘加载（恢复时克隆一份，保留缓存供下次切回使用）
        match new_mode {
            AppMode::Default => {
                if let Some(ref c) = self.cached_default_cards {
                    self.cards = c.clone();
                } else {
                    self.start_async_load();
                }
            }
            AppMode::Inspiration => {
                if let Some(ref c) = self.cached_inspiration_cards {
                    self.cards = c.clone();
                } else {
                    self.start_async_load();
                }
            }
            AppMode::Board => {
                if let Some(ref c) = self.cached_board_cards {
                    self.cards = c.clone();
                    self.apply_board_layout_if_present();
                } else {
                    self.start_async_load();
                }
            }
        }

        self.initial_arrange_done = false;

        #[cfg(windows)]
        {
            self._win_drop_guard = None;
            self.win_drop_rx = None;
            self.win_drop_pending_hwnd = self.win_drop_hwnd;
        }
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

    /// 启动时或切换模式后，在后台线程扫描图片并发送 (path, dimensions, metadata)；主线程在 poll_loading_cards 中消费。
    fn start_async_load(&mut self) {
        // 若有未结束的上次加载，先回收再启动新加载
        if let Some(rx) = self.loading_rx.take() {
            drop(rx);
        }
        if let Some(h) = self.loading_handle.take() {
            let _ = h.join();
        }

        let mode = self.mode;
        let watch_path = PathBuf::from(&self.watch_path);
        let screenshot_dir = self.screenshot_dir.clone();
        let extended_formats = self.preview_professional_formats;
        let repaint = self.repaint_ctx.clone();
        let (tx, rx) = mpsc::channel();

        let handle = thread::Builder::new()
            .name("deckref-loader".into())
            .spawn(move || {
                crate::app_threads::run_loader_thread(
                    mode,
                    watch_path,
                    screenshot_dir,
                    extended_formats,
                    MAX_CARDS,
                    INSPIRATION_RECENT_MAX,
                    tx,
                    repaint,
                );
            });

        match handle {
            Ok(h) => {
                self.loading_rx = Some(rx);
                self.loading_handle = Some(h);
            }
            Err(e) => {
                eprintln!("[DeckRef] loader thread spawn failed: {}, falling back to sync load", e);
                self.load_existing_images();
            }
        }
    }

    /// 默认/画板模式下切换到指定目录：更新监听路径、重启 watcher、重新加载该目录图片。仅当路径存在且为目录时生效。
    /// `add_to_history`：为 true 时写入前进/返回历史（点击面包屑、选子目录）；为 false 时仅跳转（后退/前进按钮）。
    fn navigate_watch_path(&mut self, new_path: PathBuf, add_to_history: bool) {
        if self.mode != AppMode::Default && self.mode != AppMode::Board {
            return;
        }
        let new_path = crate::app_nav::normalize_watch_path_for_nav(new_path);
        if !new_path.is_dir() {
            return;
        }
        if add_to_history {
            self.nav_history.truncate(self.nav_index + 1);
            self.nav_history.push(new_path.clone());
            self.nav_index = self.nav_history.len() - 1;
            self.push_recent_folder(new_path.clone());
        }
        self.apply_watch_path(new_path);
    }

    /// 将路径加入「最近到访」列表（最多 5 个），超出时从 recent_folder_cards 移除最旧项。
    fn push_recent_folder(&mut self, path: PathBuf) {
        self.recent_folders.retain(|p| *p != path);
        self.recent_folders.insert(0, path);
        if self.recent_folders.len() > RECENT_FOLDERS_MAX {
            if let Some(removed) = self.recent_folders.pop() {
                self.recent_folder_cards.remove(&removed);
            }
        }
    }

    /// 将当前监听目录设为指定路径并重新加载（watcher、卡片、Windows 拖放）。不修改导航历史。
    /// 若目标路径在 recent_folder_cards 中则直接恢复卡片，否则清空并异步加载。
    fn apply_watch_path(&mut self, watch_path: PathBuf) {
        let old_path = PathBuf::from(&self.watch_path);
        // 画板模式切换目录前保存当前目录布局，并同步到默认/画板两处路径（易用性：两模式文件夹同步）
        if self.mode == AppMode::Board && !self.cards.is_empty() {
            let canvas_size = self.layout_reference_size.unwrap_or(self.last_stack_size);
            let _ = layout::save_layout(&old_path, &self.cards, canvas_size);
            let _ = window_state::save_last_folder_sync(&old_path);
        }
        if (self.mode == AppMode::Default || self.mode == AppMode::Board) && self.recent_folders.iter().any(|p| *p == old_path) {
            self.recent_folder_cards.insert(old_path, self.cards.clone());
        }

        if let Ok((rx, handle)) = crate::app_threads::spawn_waking_watcher(
            watch_path.clone(),
            self.preview_professional_formats,
            self.repaint_ctx.clone(),
        ) {
            self._watcher_handle = Some(handle);
            self.watch_rx = rx;
        }
        self.watch_generation.fetch_add(1, Ordering::Relaxed);
        self.watch_io_inflight = 0;
        self.watch_path = watch_path.display().to_string();
        if (self.mode == AppMode::Default || self.mode == AppMode::Board) && watch_path.is_dir() {
            let _ = window_state::save_last_folder_sync(&watch_path);
        }
        self.long_press = None;
        self.picked_up = None;
        self.held_smooth_pos = None;
        self.held_scale = None;
        self.drag_pending = None;
        self.selected_indices.clear();
        self.marquee_start_screen = None;
        self.marquee_last_end_screen = None;

        self.initial_arrange_done = false;

        if let Some(cached) = self.recent_folder_cards.remove(&watch_path) {
            self.cards = cached;
            if self.mode == AppMode::Board {
                self.apply_board_layout_if_present();
            }
            if let Some(rx) = self.loading_rx.take() {
                drop(rx);
            }
            if let Some(h) = self.loading_handle.take() {
                let _ = h.join();
            }
        } else {
            self.cards.clear();
            self.texture_cache.clear();
            self.texture_cache_scope.clear();
            self.pending_texture_paths.clear();
            if let Some(rx) = self.loading_rx.take() {
                drop(rx);
            }
            if let Some(h) = self.loading_handle.take() {
                let _ = h.join();
            }
            self.start_async_load();
        }

        #[cfg(windows)]
        {
            self._win_drop_guard = None;
            self.win_drop_rx = None;
            self.win_drop_pending_hwnd = self.win_drop_hwnd;
        }
    }

    /// 画板模式：若当前目录存在 .deckref_layout.json 则加载并应用到 cards（顺序与每卡 offset/rotation/scale）；
    /// 若布局带 reference_available_size 则设置 layout_reference_size，首帧绘制时再按当前画布尺寸缩放。
    fn apply_board_layout_if_present(&mut self) {
        if self.mode != AppMode::Board || self.cards.is_empty() {
            return;
        }
        let watch_path = PathBuf::from(&self.watch_path);
        if let Some(layout) = layout::load_layout(&watch_path) {
            layout::apply_layout_to_cards(&watch_path, &mut self.cards, &layout, None);
            if let Some(ref_sz) = layout.reference_available_size {
                self.layout_reference_size = Some(egui::Vec2::new(ref_sz[0], ref_sz[1]));
            }
        }
    }

    /// 默认模式下后退到上一个目录；无历史则无操作。
    fn nav_back(&mut self) {
        if (self.mode != AppMode::Default && self.mode != AppMode::Board) || self.nav_index == 0 {
            return;
        }
        self.nav_index -= 1;
        let path = self.nav_history[self.nav_index].clone();
        self.apply_watch_path(path);
    }

    /// 默认模式下前进到下一个目录；无历史则无操作。
    fn nav_forward(&mut self) {
        if self.mode != AppMode::Default && self.mode != AppMode::Board {
            return;
        }
        if self.nav_index >= self.nav_history.len().saturating_sub(1) {
            return;
        }
        self.nav_index += 1;
        let path = self.nav_history[self.nav_index].clone();
        self.apply_watch_path(path);
    }

    /// 每帧消费异步加载结果，将收到的卡片加入列表；channel 关闭后 join 加载线程。
    fn poll_loading_cards(&mut self) {
        let Some(ref rx) = self.loading_rx else {
            return;
        };
        let max = if self.mode == AppMode::Inspiration {
            INSPIRATION_RECENT_MAX
        } else {
            MAX_CARDS
        };
        loop {
            match rx.try_recv() {
                Ok((path, (w, h), (file_size, modified))) => {
                    if self.cards.len() < max {
                        let intrinsic = Vec2::new(w as f32, h as f32);
                        let card = ImageCard::from_path_with_metadata(
                            path,
                            intrinsic,
                            file_size,
                            modified,
                        );
                        self.cards.push(card);
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.loading_rx = None;
                    if let Some(h) = self.loading_handle.take() {
                        let _ = h.join();
                    }
                    if self.mode == AppMode::Board {
                        self.apply_board_layout_if_present();
                    }
                    self.initial_arrange_done = false;
                    break;
                }
            }
        }
    }

    /// 启动时或切换模式后扫描当前模式的图片并加入列表（同步版本，已由 start_async_load 替代；保留供测试或回退）。
    #[allow(dead_code)]
    fn load_existing_images(&mut self) {
        let paths: Vec<PathBuf> = if self.mode == AppMode::Inspiration {
            #[cfg(windows)]
            {
                crate::win_recent::list_recent_image_paths(INSPIRATION_RECENT_MAX, self.preview_professional_formats)
            }
            #[cfg(not(windows))]
            {
                let watch_path = std::path::PathBuf::from(&self.watch_path);
                list_existing_image_paths(&watch_path, self.preview_professional_formats).unwrap_or_default()
            }
        } else {
            let watch_path = std::path::PathBuf::from(&self.watch_path);
            match list_existing_image_paths(&watch_path, self.preview_professional_formats) {
                Ok(p) => p,
                Err(_) => return,
            }
        };
        let max = if self.mode == AppMode::Inspiration {
            INSPIRATION_RECENT_MAX
        } else {
            MAX_CARDS
        };
        for path in paths {
            if self.cards.len() >= max {
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
            let generation = self.watch_generation.load(Ordering::Relaxed);
            if let Some(tx) = &self.watch_io_tx {
                if tx.send((generation, path.clone())).is_ok() {
                    self.watch_io_inflight = self.watch_io_inflight.saturating_add(1);
                    continue;
                }
            }
            self.push_watched_card_blocking(path);
        }
        self.poll_watch_io_results();
    }

    fn poll_watch_io_results(&mut self) {
        let current = self.watch_generation.load(Ordering::Relaxed);
        let mut ready = Vec::new();
        let mut disconnected = false;
        if let Some(rx) = self.watch_io_rx.as_ref() {
            loop {
                match rx.try_recv() {
                    Ok(done) => ready.push(done),
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
        }
        for done in ready {
            if done.generation != current {
                continue;
            }
            self.watch_io_inflight = self.watch_io_inflight.saturating_sub(1);
            self.push_watched_card(done);
        }
        if disconnected {
            self.watch_io_rx = None;
            self.watch_io_tx = None;
            self.watch_io_inflight = 0;
        }
    }

    fn push_watched_card(&mut self, done: crate::app_threads::WatchIoDone) {
        let display_size = done
            .dimensions
            .map(|(w, h)| Vec2::new(w as f32, h as f32))
            .unwrap_or(Vec2::new(220.0, 220.0));
        let mut card = ImageCard::from_path_with_metadata(
            done.path.clone(),
            display_size,
            done.file_size,
            done.modified,
        );
        if self.recent_drop_paths.remove(&done.path) {
            card.anim_position_delta = Vec2::new(0.0, DROP_ANIM_INITIAL_OFFSET_Y);
            card.anim_scale = DROP_ANIM_INITIAL_SCALE;
        }
        self.push_card_evicting(card);
    }

    /// worker 不可用时的回退。正常路径不会走到这里。
    fn push_watched_card_blocking(&mut self, path: PathBuf) {
        let display_size = image_ops::image_dimensions(&path)
            .map(|(w, h)| Vec2::new(w as f32, h as f32))
            .unwrap_or(Vec2::new(220.0, 220.0));
        let mut card = ImageCard::from_path(path.clone(), display_size);
        if self.recent_drop_paths.remove(&path) {
            card.anim_position_delta = Vec2::new(0.0, DROP_ANIM_INITIAL_OFFSET_Y);
            card.anim_scale = DROP_ANIM_INITIAL_SCALE;
        }
        self.push_card_evicting(card);
    }

    fn push_card_evicting(&mut self, card: ImageCard) {
        self.cards.push(card);
        let max = if self.mode == AppMode::Inspiration {
            INSPIRATION_RECENT_MAX
        } else {
            MAX_CARDS
        };
        if self.cards.len() > max {
            let removed = self.cards.remove(0);
            self.texture_cache.remove(&removed.path);
        }
    }

    /// 轮询剪贴板：若检测到与上次不同的图片则保存到截图目录（screenshot-N.png），灵感模式下即时加入卡片列表。
    /// 仅当「Shift 按下后的 N 秒内」才保存，避免误存 QQ/浏览器等复制的图片；闲暇时仍会更新剪贴板哈希以便进入监听窗口时不误存旧内容。
    fn poll_clipboard_screenshot(&mut self, ctx: &egui::Context) {
        let t = ctx.input(|i| i.time);
        // 按下 Shift 即开启/延长「截图监听窗口」，便于 Win+Shift+S 等操作后自动保存
        if ctx.input(|i| i.modifiers.shift) {
            self.shift_listening_until = t + SHIFT_LISTENING_DURATION;
        }
        if t - self.last_clipboard_check_time < CLIPBOARD_POLL_INTERVAL {
            return;
        }
        self.last_clipboard_check_time = t;

        let mut clipboard = match arboard::Clipboard::new() {
            Ok(c) => c,
            Err(_) => return,
        };
        let img = match clipboard.get_image() {
            Ok(d) => d,
            Err(_) => {
                self.last_clipboard_image_hash = None;
                return;
            }
        };

        let bytes = &img.bytes;
        let mut hasher = std::hash::DefaultHasher::new();
        bytes.hash(&mut hasher);
        let hash = hasher.finish();

        let prev = self.last_clipboard_image_hash.replace(hash);
        if prev.is_none() {
            return;
        }
        if prev == Some(hash) {
            return;
        }

        // 仅在与「Shift 按下后的监听窗口」内才保存，否则只更新哈希、不写入磁盘
        if t > self.shift_listening_until {
            return;
        }

        let _ = std::fs::create_dir_all(&self.screenshot_dir);
        let path = image_ops::next_screenshot_path(&self.screenshot_dir);
        let w = img.width.try_into().unwrap_or(0u32);
        let h = img.height.try_into().unwrap_or(0u32);
        if w == 0 || h == 0 {
            return;
        }
        if image_ops::save_rgba_as_png(w, h, bytes, &path).is_err() {
            return;
        }

        if self.mode == AppMode::Inspiration {
            let intrinsic = Vec2::new(w as f32, h as f32);
            let mut card = ImageCard::from_path(path.clone(), intrinsic);
            card.anim_position_delta = Vec2::new(0.0, DROP_ANIM_INITIAL_OFFSET_Y);
            card.anim_scale = DROP_ANIM_INITIAL_SCALE;
            self.cards.push(card);
            if self.cards.len() > INSPIRATION_RECENT_MAX {
                let removed = self.cards.remove(0);
                self.texture_cache.remove(&removed.path);
            }
            ctx.request_repaint();
        }
    }

    /// 处理本帧内拖入窗口的文件：已在监听目录的图只置顶不复制；刚拖出又拖回的同一张图只置顶不复制，防止循环保存；新图复制/保存并标记落下动画。
    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        if dropped.is_empty() {
            return;
        }
        let watch_dir = std::path::PathBuf::from(&self.watch_path);
        for file in dropped {
            let saved = if let Some(ref path) = file.path {
                if !image_ops::is_image_path_with(path, self.preview_professional_formats) {
                    None
                } else {
                    let dropped_canon = path.canonicalize().ok();
                    // 刚拖出又拖回的同一张图：只置顶不保存，防止循环保存
                    if let Some(ref canon) = dropped_canon {
                        if self.recent_drag_out_paths.remove(canon) {
                            let idx = self.cards.iter().position(|c| {
                                c.path.canonicalize().ok().as_ref() == Some(canon)
                            });
                            if let Some(i) = idx {
                                self.bring_to_front(i);
                                ctx.request_repaint();
                            }
                            None
                        } else if image_ops::path_is_under(path, &watch_dir) {
                            // 已在监听目录：找到对应卡片并置顶，不复制
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
                    } else if image_ops::path_is_under(path, &watch_dir) {
                        // canonicalize 失败时无法按规范路径匹配，仅不复制
                        None
                    } else {
                        image_ops::copy_image_to_dir(path, &watch_dir).ok()
                    }
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

    /// 处理单条“拖入路径”（来自 Windows 原生拖放或 egui 的 path）：刚拖出又拖回则只置顶不复制；已在监听目录则置顶，否则复制并标记落下动画。
    fn handle_dropped_path(&mut self, path: PathBuf, ctx: &egui::Context) {
        let watch_dir = std::path::PathBuf::from(&self.watch_path);
        if !image_ops::is_image_path_with(&path, self.preview_professional_formats) {
            return;
        }
        if let Ok(canon) = path.canonicalize() {
            if self.recent_drag_out_paths.remove(&canon) {
                let idx = self.cards.iter().position(|c| {
                    c.path.canonicalize().ok().as_ref() == Some(&canon)
                });
                if let Some(i) = idx {
                    self.bring_to_front(i);
                    ctx.request_repaint();
                }
                return;
            }
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

    fn cached_texture_edge(&self, path: &std::path::Path) -> u32 {
        self.texture_cache
            .get(path)
            .map(|tex| {
                let [w, h] = tex.size();
                w.max(h) as u32
            })
            .unwrap_or(0)
    }

    /// 确保纹理已加载到至少 `max_edge`；已有更小的缩略图时先返回它，并在后台换更大的一档。
    /// 优先走后台解码线程，主线程仅在 poll_texture_results 中做 GPU 上传。
    /// 无后台线程时的回退仍用缩略图，主线程绝不完整解码超大图。
    fn ensure_texture(
        &mut self,
        ctx: &egui::Context,
        path: &PathBuf,
        max_edge: u32,
    ) -> Option<&TextureHandle> {
        let max_edge = max_edge.max(1);
        let cached_edge = self.cached_texture_edge(path);
        if cached_edge >= max_edge {
            return self.texture_cache.get(path);
        }
        if let Some(tx) = self.texture_loading_tx.clone() {
            let pending_edge = self.pending_texture_paths.get(path).copied().unwrap_or(0);
            if pending_edge < max_edge {
                let _ = tx.send((path.clone(), max_edge));
                self.pending_texture_paths.insert(path.clone(), max_edge);
            }
            return self.texture_cache.get(path);
        }
        // 回退：无后台线程时同步解码。超大图只走 JPEG/WebP 的解码期缩小。
        let img = match image_ops::image_dimensions(path) {
            Ok((w, h)) if w.max(h) > max_edge && !image_ops::subsamples_when_downscaling(path) && w.max(h) > image_ops::THUMBNAIL_RESOLUTION_THRESHOLD => {
                return self.texture_cache.get(path);
            }
            _ => image_ops::load_image_thumbnail(path, max_edge).ok(),
        }?;
        let size = [img.width() as usize, img.height() as usize];
        let rgba = img.to_rgba8();
        let color_image = ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
        let name = path.to_string_lossy().to_string();
        let handle = ctx.load_texture(name, color_image, Default::default());
        self.texture_cache.insert(path.clone(), handle);
        if let Some(folder) = path.parent() {
            let folder = folder.to_path_buf();
            self.texture_cache_scope.retain(|p| p != &folder);
            self.texture_cache_scope.insert(0, folder.clone());
            while self.texture_cache_scope.len() > MAX_TEXTURE_CACHE_FOLDERS {
                if let Some(old) = self.texture_cache_scope.pop() {
                    self.texture_cache.retain(|k, _| !k.starts_with(&old));
                }
            }
        }
        self.texture_cache.get(path)
    }

    /// 每帧从后台纹理解码线程取若干已完成结果，仅对仍属于当前卡片的 path 做 GPU 上传并写入缓存；每帧上限以平滑 UI。
    fn poll_texture_results(&mut self, ctx: &egui::Context) {
        const UPLOADS_PER_FRAME: usize = 5;
        let Some(ref rx) = self.texture_loading_rx else {
            return;
        };
        let mut uploaded = 0;
        while uploaded < UPLOADS_PER_FRAME {
            let Ok((path, rgba, width, height)) = rx.try_recv() else {
                break;
            };
            let got_edge = width.max(height);
            if self.pending_texture_paths.get(&path).copied().unwrap_or(0) <= got_edge {
                self.pending_texture_paths.remove(&path);
            }
            if !self.cards.iter().any(|c| c.path == path) {
                continue;
            }
            if self.cached_texture_edge(&path) >= got_edge {
                continue;
            }
            let size = [width as usize, height as usize];
            let color_image = ColorImage::from_rgba_unmultiplied(size, &rgba);
            let name = path.to_string_lossy().to_string();
            let handle = ctx.load_texture(name, color_image, Default::default());
            self.texture_cache.insert(path.clone(), handle);
            if let Some(folder) = path.parent() {
                let folder = folder.to_path_buf();
                self.texture_cache_scope.retain(|p| p != &folder);
                self.texture_cache_scope.insert(0, folder.clone());
                while self.texture_cache_scope.len() > MAX_TEXTURE_CACHE_FOLDERS {
                    if let Some(old) = self.texture_cache_scope.pop() {
                        self.texture_cache.retain(|k, _| !k.starts_with(&old));
                    }
                }
            }
            uploaded += 1;
        }
        if uploaded > 0 {
            ctx.request_repaint();
        }
    }

    /// 若存在 assets/icon.png 则加载为标题栏 logo 纹理，只尝试一次，失败不重试。（当前未使用）
    #[allow(dead_code)]
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
        if self.mode == AppMode::Board {
            self.board_view_scale = 1.0;
            self.board_view_offset = egui::Vec2::ZERO;
        }
        self.grid_view = false;
    }

    /// 阵列排列：按原比例无缝紧密排列（类似股市热力图），行内无缝隙、行间无缝隙，填满可用区域。
    fn grid_arrange(&mut self) {
        let n = self.cards.len();
        if n == 0 {
            return;
        }
        let size = self.stack_layout.available_size;
        let pad = 4.0;
        let inner_w = (size.x - pad * 2.0).max(100.0);
        let inner_h = (size.y - pad * 2.0).max(100.0);
        let half_w = inner_w * 0.5;
        let half_h = inner_h * 0.5;

        // 每张图的原比例 aspect = w/h
        let aspects: Vec<f32> = self
            .cards
            .iter()
            .map(|c| {
                let (iw, ih) = (c.intrinsic_size.x.max(1.0), c.intrinsic_size.y.max(1.0));
                iw / ih
            })
            .collect();
        let total_aspect: f32 = aspects.iter().sum();

        if total_aspect <= 0.0 {
            return;
        }

        // 估计行数：总高度 ≈ inner_h => R ≈ sqrt(inner_h * S / inner_w)
        let r_f = (inner_h * total_aspect / inner_w).sqrt().max(1.0);
        let num_rows = r_f.ceil().min(n as f32).max(1.0) as usize;
        let target_sum_per_row = total_aspect / num_rows as f32;

        // 贪心分行：每行 sum(aspect) 尽量接近 target，最后一行收尾到 n
        let mut row_starts = vec![0usize];
        let mut row_sum = 0.0f32;
        for i in 0..n {
            let a = aspects[i];
            if row_starts.len() < num_rows
                && row_sum >= target_sum_per_row * 0.5
                && (row_sum + a > target_sum_per_row * 1.2 || row_starts.len() == num_rows - 1)
            {
                row_starts.push(i);
                row_sum = 0.0;
            }
            row_sum += a;
        }
        if row_starts.last().copied().unwrap_or(0) < n {
            row_starts.push(n);
        }
        let num_rows_actual = (row_starts.len() - 1).max(1);
        let row_boundaries = &row_starts[..];

        // 每行高度（未缩放）：row_height = inner_w / sum(aspect in row)
        let mut row_heights: Vec<f32> = Vec::with_capacity(num_rows_actual);
        for r in 0..num_rows_actual {
            let start = row_boundaries[r];
            let end = row_boundaries.get(r + 1).copied().unwrap_or(n);
            let sum_r: f32 = aspects[start..end].iter().sum();
            row_heights.push(inner_w / sum_r.max(1e-6));
        }
        let total_height: f32 = row_heights.iter().sum();
        let scale = inner_h / total_height.max(1e-6);

        // 为每张卡计算世界坐标中心与显示尺寸，并写回 random_offset / display_scale
        for r in 0..num_rows_actual {
            let start = row_boundaries[r];
            let end = row_boundaries.get(r + 1).copied().unwrap_or(n);
            let row_h = row_heights[r] * scale;
            let y_center = -half_h + row_h * 0.5
                + (0..r).map(|rr| row_heights[rr] * scale).sum::<f32>();

            let mut x = -half_w;
            for i in start..end {
                let card = &mut self.cards[i];
                let a = aspects[i];
                let w = a * row_h;
                let x_center = x + w * 0.5;
                x += w;

                let (layout_pos, layout_rot) = self.stack_layout.position_and_rotation(i, n);
                let layout_pos = layout_pos.to_vec2();
                let world_center = egui::Vec2::new(x_center, y_center);
                let new_offset = world_center - layout_pos;

                card.anim_position_delta = card.random_offset - new_offset;
                card.random_offset = new_offset;
                card.anim_rotation_offset = 0.0;
                card.random_rotation = -layout_rot;

                let effective = self.stack_layout.effective_display_size(card.intrinsic_size, i, n);
                let eff_x = effective.x.max(1e-6);
                let eff_y = effective.y.max(1e-6);
                let display_scale = (w / eff_x).min(row_h / eff_y);
                card.display_scale = display_scale.clamp(DISPLAY_SCALE_MIN, DISPLAY_SCALE_MAX);
                // 画板模式：将阵列的世界坐标写入参考空间，使显示使用 board_saved_world_pos 且与保存一致
                if self.mode == AppMode::Board {
                    card.board_saved_world_pos = Some(world_center);
                }
            }
        }

        if self.mode == AppMode::Board {
            self.layout_reference_size = Some(self.stack_layout.available_size);
            self.board_view_scale = 1.8;
            self.board_view_offset = egui::Vec2::ZERO;
        }

        self.view_scale = 1.8;
        self.view_offset = egui::Vec2::ZERO;
        self.grid_view = true;
    }

    fn run_ui(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if let Ok(mut guard) = self.repaint_ctx.lock() {
            *guard = Some(ctx.clone());
        }
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
                    self.win_drop_hwnd = Some(hwnd);
                }
                Err(e) => {
                    eprintln!("[DeckRef] RegisterDragDrop failed: {}", e);
                }
            }
        }

        // 关闭请求：保存当前窗口位置与尺寸后关闭，下次启动将按相同位置与尺寸打开（含所在显示器）
        if ctx.input(|i| i.viewport().close_requested()) {
            let inner_rect = ctx.screen_rect();
            let outer_pos = ctx.input(|i| i.viewport().outer_rect.map(|r| r.min));
            if let Some(pos) = outer_pos {
                let _ = crate::window_state::save_window_state(
                    pos.x as f64,
                    pos.y as f64,
                    inner_rect.width() as f64,
                    inner_rect.height() as f64,
                );
            } else {
                let _ = crate::window_state::save_window_state(
                    0.0,
                    0.0,
                    inner_rect.width() as f64,
                    inner_rect.height() as f64,
                );
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        // 按住右键拖动移动窗口（参考 PureRef：超过阈值才视为移动；Windows 上有 hwnd 时桥接系统标题栏拖动，无抖动）
        {
            let (secondary_down, delta, outer_rect) = ctx.input(|i| {
                (
                    i.pointer.secondary_down(),
                    i.pointer.delta(),
                    i.viewport().outer_rect.map(|r| r.min),
                )
            });
            if secondary_down {
                if self.right_drag_started {
                    // 已进入移动：Windows 桥接会阻塞直到松开，不会走到这里；非 Windows 或无 hwnd 时用手动
                    if let Some(last) = self.right_drag_last_sent {
                        let target = last + delta;
                        let rounded = egui::Pos2::new(target.x.round(), target.y.round());
                        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(rounded));
                        self.right_drag_last_sent = Some(rounded);
                    }
                } else {
                    self.right_drag_pending_delta += delta;
                    let dist_sq = self.right_drag_pending_delta.length_sq();
                    if dist_sq >= RIGHT_DRAG_MOVE_THRESHOLD * RIGHT_DRAG_MOVE_THRESHOLD {
                        self.right_drag_started = true;
                        #[cfg(windows)]
                        if let Some(hwnd) = self.win_drop_hwnd {
                            crate::win_move::start_system_window_drag(hwnd);
                            // 桥接返回后手势已结束，重置以便下次右键拖动
                            self.right_drag_pending_delta = egui::Vec2::ZERO;
                            self.right_drag_started = false;
                            self.right_drag_last_sent = None;
                        } else {
                            if let Some(pos) = outer_rect {
                                let target = pos + self.right_drag_pending_delta;
                                let rounded = egui::Pos2::new(target.x.round(), target.y.round());
                                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(rounded));
                                self.right_drag_last_sent = Some(rounded);
                            }
                            self.right_drag_pending_delta = egui::Vec2::ZERO;
                        }
                        #[cfg(not(windows))]
                        {
                            if let Some(pos) = outer_rect {
                                let target = pos + self.right_drag_pending_delta;
                                let rounded = egui::Pos2::new(target.x.round(), target.y.round());
                                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(rounded));
                                self.right_drag_last_sent = Some(rounded);
                            }
                            self.right_drag_pending_delta = egui::Vec2::ZERO;
                        }
                    }
                }
            } else {
                self.right_drag_pending_delta = egui::Vec2::ZERO;
                self.right_drag_started = false;
                self.right_drag_last_sent = None;
            }
        }

        self.poll_watch_events();

        // 消费异步加载线程发来的卡片（启动或切换模式后）
        self.poll_loading_cards();

        // 消费后台纹理解码结果，每帧最多上传若干张，避免单帧卡顿
        self.poll_texture_results(ctx);

        // 运行时监听到剪贴板截图：仅当「按下 Shift 后的约 5 秒内」且剪贴板图片与上次不同时，保存到 Ref/Screenshots（screenshot-N.png），灵感模式下即时显示；否则不保存，避免误存 QQ/浏览器等复制的图
        self.poll_clipboard_screenshot(ctx);

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

        ui_mode_sidebar::show_mode_sidebar(self, ctx);

        // 底部栏改为叠加层，与地址栏一起在图片上层（先不占位，由后面的 CentralPanel 占满）
        // 堆叠区域（仅卡片；地址栏与底部栏在顶层 Area 叠加，不被图片遮挡）
        egui::CentralPanel::default()
            .frame(egui::Frame::none())
            .show(ctx, |ui| {
                ui_stack_canvas::show(self, ctx, ui, dt);
            });

        ui_top_bar::show_top_bar(self, ctx);
        ui_bottom_bar::show_bottom_bar(self, ctx);

        // 设置窗口（右上角齿轮打开）
        egui::Window::new("设置")
            .open(&mut self.settings_open)
            .collapsible(false)
            .resizable(false)
            .default_width(260.0)
            .show(ctx, |ui| {
                let mut changed = false;
                let mut reload_after = false;
                if ui
                    .checkbox(&mut self.invert_pan, "反转平移方向（中键拖动）")
                    .on_hover_text("开启后，中键拖动时视角移动方向与默认相反")
                    .changed()
                {
                    changed = true;
                }
                if ui
                    .checkbox(
                        &mut self.allow_drag_out,
                        "贴图拖拽出窗口",
                    )
                    .on_hover_text("开启后，拿起图片拖出窗口边缘可拖到文件夹或其它软件；关闭则仅在窗口内移动")
                    .changed()
                {
                    changed = true;
                }
                if ui
                    .checkbox(
                        &mut self.preview_professional_formats,
                        "预览专业格式图片",
                    )
                    .on_hover_text("开启后可预览 TGA、ICO、DDS、AVIF、EXR、HDR、PNM、QOI、PSD 等格式；更改后会重新加载当前目录")
                    .changed()
                {
                    changed = true;
                    reload_after = true;
                }
                if ui
                    .checkbox(
                        &mut self.autodesk_style_selection,
                        "AutoDesk 式选取（框选）",
                    )
                    .on_hover_text("开启：从左向右拖 = 仅完全落入选框的图片被选中；从右向左拖 = 沾到即选中。关闭：统一为沾到即选中（Windows 风格）")
                    .changed()
                {
                    changed = true;
                }
                if changed {
                    let s = settings::AppSettings {
                        invert_pan: self.invert_pan,
                        allow_drag_out: self.allow_drag_out,
                        preview_professional_formats: self.preview_professional_formats,
                        autodesk_style_selection: self.autodesk_style_selection,
                    };
                    let _ = settings::save_settings(&s);
                    if reload_after {
                        self.pending_reload_after_settings = true;
                    }
                }
            });
        if self.pending_reload_after_settings {
            self.pending_reload_after_settings = false;
            self.apply_watch_path(std::path::PathBuf::from(&self.watch_path));
        }

        // 长按拿起后指针离开窗口边缘：若允许则发起系统文件拖放；否则仅取消拿起状态。两种情况下都清除拿起/拖出状态。
        let screen_rect = ctx.screen_rect();
        let pointer = ctx.input(|i| i.pointer.hover_pos());
        let pointer_outside = pointer.map(|p| !screen_rect.contains(p)).unwrap_or(true);
        if pointer_outside && self.picked_up.is_some() {
            let idx = self.picked_up.as_ref().map(|(i, _, _)| *i).unwrap_or(0);
            if self.allow_drag_out {
                if let Some(card) = self.cards.get(idx) {
                    let path = card.path.clone();
                    let canonical = std::fs::canonicalize(&path).ok();
                    let paths = canonical
                        .as_ref()
                        .map(|p| vec![p.clone()])
                        .unwrap_or_else(|| vec![path.clone()]);
                    // 记录「刚拖出」路径，拖回同一张图时只置顶不复制，防止循环保存
                    let to_record = canonical.as_ref().unwrap_or(&path);
                    if self.recent_drag_out_paths.len() >= MAX_DRAG_OUT_RECENT {
                        self.recent_drag_out_paths.clear();
                    }
                    self.recent_drag_out_paths.insert(to_record.clone());
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
            }
            self.drag_pending = None;
            self.long_press = None;
            self.picked_up = None;
            self.held_smooth_pos = None;
            self.held_scale = None;
        }

        // 画板模式：每约 5 秒防抖写入一次布局到 .deckref_layout.json
        if self.mode == AppMode::Board && !self.cards.is_empty() {
            let now = ctx.input(|i| i.time);
            if self.last_board_layout_save_time == 0.0 {
                self.last_board_layout_save_time = now;
            } else if now - self.last_board_layout_save_time >= 5.0 {
                let path = std::path::Path::new(&self.watch_path);
                if path.is_dir() {
                    let canvas_size = self.layout_reference_size.unwrap_or(self.last_stack_size);
                    let _ = layout::save_layout(path, &self.cards, canvas_size);
                    self.last_board_layout_save_time = now;
                }
            }
        }

        // 静止且没有未完成工作时不主动重绘，交给 egui 的输入事件。
        // 平移/缩放写在收到滚轮或中键拖动的那一帧里，松手后保持当前 view，不必空转。
        let now = ctx.input(|i| i.time);
        let interacting = self.picked_up.is_some()
            || self.long_press.is_some()
            || self.right_drag_started
            || self.drag_pending.is_some()
            || self.marquee_start_screen.is_some()
            || ctx.input(|i| i.pointer.middle_down());
        let loading = self.loading_rx.is_some()
            || self.watch_io_inflight > 0
            || !self.pending_texture_paths.is_empty();
        if interacting || loading || self.card_motion_pending() {
            ctx.request_repaint();
        } else if now < self.shift_listening_until {
            let since_check = now - self.last_clipboard_check_time;
            let wait = (CLIPBOARD_POLL_INTERVAL - since_check).max(0.05);
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(wait));
        } else if self.mode == AppMode::Board && !self.cards.is_empty() {
            let elapsed = now - self.last_board_layout_save_time;
            if self.last_board_layout_save_time > 0.0 && elapsed < 5.0 {
                ctx.request_repaint_after(std::time::Duration::from_secs_f64((5.0 - elapsed).max(0.05)));
            }
        }
    }

    /// 落下/打散/悬停缩放尚未收敛时需要继续重绘。悬停停在 1.08 视为已收敛。
    fn card_motion_pending(&self) -> bool {
        let hovered = self.last_hovered_index;
        let dropped = self.recently_dropped_index;
        self.cards.iter().enumerate().any(|(index, card)| {
            let target_scale = if hovered == Some(index) { 1.08 } else { 1.0 };
            (card.anim_scale - target_scale).abs() > 1e-3
                || card.anim_position_delta.length_sq() > 1e-4
                || (dropped == Some(index) && card.anim_rotation_offset.abs() > 5e-4)
        })
    }
}

impl Drop for DeckRefApp {
    fn drop(&mut self) {
        if self.mode == AppMode::Default {
            if !self.watch_path.is_empty() && self.watch_path != THIS_PC_SENTINEL {
                let path = std::path::Path::new(&self.watch_path);
                if path.is_dir() {
                    let _ = window_state::save_last_folder_sync(path);
                }
            }
            let _ = window_state::save_recent_folders(&self.recent_folders);
        }
        if self.mode == AppMode::Board {
            if !self.watch_path.is_empty() && !self.cards.is_empty() {
                let path = std::path::Path::new(&self.watch_path);
                let canvas_size = self.layout_reference_size.unwrap_or(self.last_stack_size);
                let _ = layout::save_layout(path, &self.cards, canvas_size);
                if path.is_dir() {
                    let _ = window_state::save_last_folder_sync(path);
                }
            }
            let _ = window_state::save_recent_folders(&self.recent_folders);
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
