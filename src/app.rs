//! UI 逻辑：堆叠照片、纹理缓存、悬停/长按/排序。

use crate::error::AppResult;
use crate::board_view;
use crate::image_card::{
    draw_card, draw_card_shadow, lerp_position, lerp_rotation, lerp_scale, normalize_angle_delta,
    rotated_rect_contains, CardSortOrder, ImageCard, StackLayout, DISPLAY_SCALE_MAX,
    DISPLAY_SCALE_MIN, DROP_ANIM_INITIAL_OFFSET_Y, DROP_ANIM_INITIAL_SCALE, HELD_FOLLOW_SMOOTH,
    PICKED_UP_LIFT, PICKED_UP_SCALE, PICK_UP_ANIM_DURATION, WHEEL_SCALE_SENSITIVITY,
};
use crate::image_ops;
use crate::mode::AppMode;
use crate::settings;
use crate::watcher::{list_existing_image_paths_with_metadata, list_existing_image_paths, WatchEvent, WatcherConfig};
use crate::window_state;
use crate::layout;
use eframe::egui::{self, emath::Rot2, ColorImage, Rect, TextureHandle, Vec2};
use rstar::{AABB, Envelope, RTree, RTreeObject};
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf, Component};
use std::ffi::OsString;
use std::sync::mpsc;
use std::thread;
use std::time::SystemTime;

/// 左侧模式切换栏宽度（两个圆角小方块 + 间距）
const MODE_SIDEBAR_WIDTH: f32 = 56.0;
const MAX_CARDS: usize = 80;
/// 灵感模式下「最近图片」上限（Windows 最近使用的文件）
const INSPIRATION_RECENT_MAX: usize = 30;
const LONG_PRESS_DURATION: f32 = 0.5;
/// 底部栏高度（单行工具栏，不抢占主内容区；桌面端鼠标点击足够）
const BOTTOM_BAR_HEIGHT: f32 = 36.0;
/// 顶部地址栏高度（叠加在图片上层，带半透明底）
const TOP_BAR_HEIGHT: f32 = 40.0;
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
/// 面包屑根「此台电脑」的占位路径，不参与真实导航，仅用于展开盘符列表（Windows）
const THIS_PC_SENTINEL: &str = "::THIS_PC::";
/// 剪贴板截图轮询间隔（秒）：至少隔这么久才读一次剪贴板，避免过于频繁。1.0～1.5 更省资源，0.8 响应更快。
const CLIPBOARD_POLL_INTERVAL: f64 = 0.8;
/// 按下 Shift 后持续监听剪贴板截图的时长（秒）。手动截屏（如 Win+Shift+S）通常伴随 Shift，此时间内剪贴板新图会保存；超时后不再自动保存，避免误存 QQ/浏览器等复制的图片。
const SHIFT_LISTENING_DURATION: f64 = 5.0;

/// Windows：列出所有逻辑盘符路径（如 C:\、D:\）。
#[cfg(windows)]
fn list_drives_windows() -> Vec<PathBuf> {
    use windows::Win32::Storage::FileSystem::GetLogicalDriveStringsW;
    let mut buf = [0u16; 256];
    unsafe {
        let len = GetLogicalDriveStringsW(Some(&mut buf[..]));
        if len == 0 {
            return Vec::new();
        }
        let s = String::from_utf16_lossy(&buf[..len as usize]);
        s.split('\0')
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .collect()
    }
}

/// 将路径解析为面包屑：(完整路径, 显示名)，用于地址栏逐级点击导航。Windows 下首段为「此台电脑」以便选盘。
fn path_breadcrumbs(path: &Path) -> Vec<(PathBuf, OsString)> {
    let mut out = Vec::new();
    let mut acc = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::Prefix(prefix) => {
                acc.push(comp);
                out.push((acc.clone(), prefix.as_os_str().to_owned()));
            }
            Component::RootDir => {
                acc.push(comp);
            }
            Component::Normal(name) => {
                acc.push(name);
                out.push((acc.clone(), name.to_owned()));
            }
            _ => {
                acc.push(comp);
            }
        }
    }
    #[cfg(windows)]
    if !out.is_empty() {
        out.insert(0, (PathBuf::from(THIS_PC_SENTINEL), OsString::from("此台电脑")));
    }
    out
}

/// 列出指定目录下的子文件夹（按名称排序，最多 limit 个）。用于面包屑每级右侧的下拉菜单。
fn list_subdirs(path: &Path, limit: usize) -> Vec<PathBuf> {
    let mut d: Vec<PathBuf> = match std::fs::read_dir(path) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .map(|e| e.path())
            .collect(),
        Err(_) => return vec![],
    };
    d.sort_by_cached_key(|p| p.file_name().unwrap_or_default().to_string_lossy().to_lowercase());
    d.truncate(limit);
    d
}

/// 面包屑单段按钮水平内边距（用于宽度估算）
const BREADCRUMB_BTN_PADDING_X: f32 = 16.0;

/// 计算面包屑在空间不足时从左侧折叠的起始索引（参考 Win11 资源管理器：保留右侧，左侧用省略号）。
/// 当窗口足够长、可用宽度能容纳完整路径时，自动展开显示全部段落（start_index = 0）。
/// 返回 (start_index, 各段宽度, 分隔符宽度, 省略号按钮宽度)。start_index > 0 时表示需显示 "…" 并只显示 segments[start_index..]。
fn breadcrumb_collapse_start(
    ui: &egui::Ui,
    segments: &[(PathBuf, std::ffi::OsString)],
    available_width: f32,
) -> (usize, Vec<f32>, f32, f32) {
    let font_id = egui::FontId::proportional(12.0);
    let color = egui::Color32::PLACEHOLDER;
    let sep_galley = ui.fonts(|f| f.layout_no_wrap(" › ".to_string(), font_id.clone(), color));
    let sep_w = sep_galley.size().x;
    let ellipsis_galley = ui.fonts(|f| f.layout_no_wrap("…".to_string(), font_id.clone(), color));
    let ellipsis_btn_w = ellipsis_galley.size().x + BREADCRUMB_BTN_PADDING_X;

    let mut seg_widths = Vec::with_capacity(segments.len());
    for (_, label) in segments.iter() {
        let s = label.to_string_lossy().to_string();
        let galley = ui.fonts(|f| f.layout_no_wrap(s, font_id.clone(), color));
        seg_widths.push(galley.size().x + BREADCRUMB_BTN_PADDING_X);
    }

    let n = segments.len();
    if n == 0 {
        return (0, seg_widths, sep_w, ellipsis_btn_w);
    }

    let full_width: f32 = seg_widths.iter().sum::<f32>() + (n as f32 - 1.0) * sep_w;
    let avail = (available_width - 4.0).max(20.0);
    if full_width <= avail {
        return (0, seg_widths, sep_w, ellipsis_btn_w);
    }

    let mut start_index = 0;
    for start in 0..n {
        let mut w = if start > 0 { ellipsis_btn_w + sep_w } else { 0.0 };
        for i in start..n {
            w += seg_widths[i];
            if i < n - 1 {
                w += sep_w;
            }
        }
        if w <= avail {
            start_index = start;
        } else {
            break;
        }
    }
    (start_index, seg_widths, sep_w, ellipsis_btn_w)
}

/// 用于视口剔除：卡片在屏幕空间的 AABB + 索引。先经 R-Tree 查询与视口相交的卡，再对查询结果做可见比例判断。
struct CardScreenRect {
    index: usize,
    aabb: AABB<[f64; 2]>,
}
impl RTreeObject for CardScreenRect {
    type Envelope = AABB<[f64; 2]>;
    fn envelope(&self) -> Self::Envelope {
        self.aabb
    }
}

/// 由世界坐标中心、半尺寸、旋转角与 world_to_screen 得到屏幕空间 AABB（用于可见比例计算）。
fn screen_aabb_for_rotated_rect(
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
fn run_loader_thread(
    mode: AppMode,
    watch_path: PathBuf,
    screenshot_dir: PathBuf,
    extended_formats: bool,
    tx: mpsc::Sender<(PathBuf, (u32, u32), (u64, SystemTime))>,
) {
    let send = |path: PathBuf, w: u32, h: u32, file_size: u64, modified: SystemTime| {
        let _ = tx.send((path, (w, h), (file_size, modified)));
    };
    match mode {
        AppMode::Default => {
            let list = list_existing_image_paths_with_metadata(&watch_path, MAX_CARDS, extended_formats)
                .unwrap_or_default();
            for (path, file_size, modified) in list {
                if let Ok((w, h)) = image_ops::image_dimensions(&path) {
                    send(path, w, h, file_size, modified);
                }
            }
        }
        AppMode::Inspiration => {
            #[cfg(windows)]
            {
                let recent = crate::win_recent::list_recent_image_paths(INSPIRATION_RECENT_MAX, extended_formats);
                let screenshots = image_ops::list_screenshot_paths_ordered(&screenshot_dir);
                let mut paths: Vec<PathBuf> = recent;
                for p in screenshots {
                    if !paths.contains(&p) && paths.len() < INSPIRATION_RECENT_MAX {
                        paths.push(p);
                    }
                }
                paths.truncate(INSPIRATION_RECENT_MAX);
                for path in paths {
                    let (file_size, modified) = std::fs::metadata(&path)
                        .map(|m| {
                            (
                                m.len(),
                                m.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                            )
                        })
                        .unwrap_or((0, SystemTime::UNIX_EPOCH));
                    if let Ok((w, h)) = image_ops::image_dimensions(&path) {
                        send(path, w, h, file_size, modified);
                    }
                }
            }
            #[cfg(not(windows))]
            {
                let mut paths: Vec<PathBuf> =
                    list_existing_image_paths(&watch_path, extended_formats).unwrap_or_default();
                for p in image_ops::list_screenshot_paths_ordered(&screenshot_dir) {
                    if !paths.contains(&p) && paths.len() < INSPIRATION_RECENT_MAX {
                        paths.push(p);
                    }
                }
                paths.truncate(INSPIRATION_RECENT_MAX);
                let list = paths
                    .into_iter()
                    .filter_map(|path| {
                        std::fs::metadata(&path).ok().map(|m| {
                            (
                                path,
                                m.len(),
                                m.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                            )
                        })
                    })
                    .collect::<Vec<_>>();
                for (path, file_size, modified) in list {
                    if let Ok((w, h)) = image_ops::image_dimensions(&path) {
                        send(path, w, h, file_size, modified);
                    }
                }
            }
        }
        AppMode::Board => {
            let list = list_existing_image_paths_with_metadata(&watch_path, MAX_CARDS, extended_formats)
                .unwrap_or_default();
            for (path, file_size, modified) in list {
                if let Ok((w, h)) = image_ops::image_dimensions(&path) {
                    send(path, w, h, file_size, modified);
                }
            }
        }
    }
    drop(tx);
}

/// 后台纹理解码线程：从 req_rx 收 path，解码为 RGBA（大图缩略图），通过 res_tx 回传；主线程只做 GPU 上传。
fn run_texture_loader_thread(
    req_rx: mpsc::Receiver<PathBuf>,
    res_tx: mpsc::Sender<(PathBuf, Vec<u8>, u32, u32)>,
) {
    for path in req_rx {
        if let Ok((rgba, w, h)) = image_ops::load_image_rgba_for_texture(&path) {
            let _ = res_tx.send((path, rgba, w, h));
        }
    }
}

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
    /// 纹理加载请求发送端：主线程只发 path，后台线程解码后通过 texture_loading_rx 回传 RGBA
    texture_loading_tx: Option<mpsc::Sender<PathBuf>>,
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
    /// 已请求但尚未收到结果的 path，避免重复请求；切换文件夹/清空时清空
    pending_texture_paths: HashSet<PathBuf>,
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
        let (tx, rx) = mpsc::channel();
        let config = WatcherConfig {
            watch_path: watch_path.clone(),
            extended_formats: loaded.preview_professional_formats,
            ..WatcherConfig::default()
        };
        let handle = crate::watcher::run_watcher(config, tx)?;
        let (tex_req_tx, tex_req_rx) = mpsc::channel();
        let (tex_res_tx, tex_res_rx) = mpsc::channel();
        let tex_handle = thread::Builder::new()
            .name("deckref-tex-loader".into())
            .spawn(move || run_texture_loader_thread(tex_req_rx, tex_res_tx));
        let (texture_loading_tx, texture_loading_rx, _texture_loading_handle) = match tex_handle {
            Ok(h) => (Some(tex_req_tx), Some(tex_res_rx), Some(h)),
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
            pending_texture_paths: HashSet::new(),
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
        let (tx, rx) = mpsc::channel();
        let config = WatcherConfig {
            watch_path: watch_path.clone(),
            extended_formats: self.preview_professional_formats,
            ..WatcherConfig::default()
        };
        if let Ok(handle) = crate::watcher::run_watcher(config, tx) {
            self._watcher_handle = Some(handle);
            self.watch_rx = rx;
        }
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
        let (tx, rx) = mpsc::channel();

        let handle = thread::Builder::new()
            .name("deckref-loader".into())
            .spawn(move || {
                run_loader_thread(mode, watch_path, screenshot_dir, extended_formats, tx);
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

        let (tx, rx) = mpsc::channel();
        let config = WatcherConfig {
            watch_path: watch_path.clone(),
            extended_formats: self.preview_professional_formats,
            ..WatcherConfig::default()
        };
        if let Ok(handle) = crate::watcher::run_watcher(config, tx) {
            self._watcher_handle = Some(handle);
            self.watch_rx = rx;
        }
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

    /// 确保纹理已加载；若失败则返回 None，不 panic。
    /// 优先走后台解码线程（请求入队、本帧返回 None），主线程仅在 poll_texture_results 中做 GPU 上传。
    /// 无后台线程时的回退路径仅在 spawn 失败时触发；回退中仍用缩略图（>2048px 用 load_image_thumbnail），主线程绝不完整解码超大图。
    fn ensure_texture(&mut self, ctx: &egui::Context, path: &PathBuf) -> Option<&TextureHandle> {
        if self.texture_cache.contains_key(path) {
            return self.texture_cache.get(path);
        }
        if let Some(ref tx) = self.texture_loading_tx {
            if self.pending_texture_paths.contains(path) {
                return None;
            }
            let _ = tx.send(path.clone());
            self.pending_texture_paths.insert(path.clone());
            return None;
        }
        // 回退：无后台线程时同步解码（避免 spawn 失败后无法显示图）
        let img = match image_ops::image_dimensions(path) {
            Ok((w, h)) if w.max(h) > image_ops::THUMBNAIL_RESOLUTION_THRESHOLD => {
                image_ops::load_image_thumbnail(path, image_ops::THUMBNAIL_MAX_LONGEST_EDGE).ok()
            }
            _ => image_ops::load_image(path).ok(),
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
            self.pending_texture_paths.remove(&path);
            if !self.cards.iter().any(|c| c.path == path) {
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

        // 左侧模式切换（默认 / 灵感）：改为叠加层，始终在图片上方
        let screen = ctx.screen_rect();
        egui::Area::new(egui::Id::new("deckref_mode_sidebar"))
            .order(egui::Order::Foreground)
            .fixed_pos(egui::Pos2::new(screen.min.x, screen.min.y + TOP_BAR_HEIGHT))
            .constrain(true)
            .show(ctx, |ui| {
                ui.set_min_size(egui::Vec2::new(
                    MODE_SIDEBAR_WIDTH,
                    screen.height() - TOP_BAR_HEIGHT - BOTTOM_BAR_HEIGHT,
                ));
                let fill = ui.visuals().window_fill();
                let c = egui::Color32::from(fill);
                let a = (c.a() as f32 * 0.04).round() as u8;
                let bg = egui::Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a);
                ui.painter().rect_filled(
                    Rect::from_min_size(egui::Pos2::ZERO, ui.available_size()),
                    egui::Rounding::same(0.0),
                    bg,
                );
                let button_block_height = 36.0 * 2.0 + 6.0;
                let top_space = (ui.available_height() - button_block_height) / 2.0;
                ui.add_space(top_space.max(0.0));
                let size = 36.0;
                let rounding = 8.0;
                for &m in AppMode::all() {
                    let selected = self.mode == m;
                    let btn = egui::Button::new(egui::RichText::new(m.label()).size(12.0))
                        .min_size(egui::Vec2::splat(size))
                        .rounding(egui::Rounding::same(rounding));
                    let mut resp = ui.add(btn);
                    if selected {
                        resp = resp.highlight();
                    }
                    if resp.clicked() {
                        self.switch_mode(m);
                    }
                    resp.on_hover_text(match m {
                        AppMode::Default => "默认模式：监听主文件夹中的图片",
                        AppMode::Inspiration => "灵感模式：展示最近使用的图片与 Ref/Screenshots 截图；按下 Shift 后约 5 秒内截屏会自动保存并显示",
                        AppMode::Board => "画板模式：画布布局持久化到当前目录的 .deckref_layout.json；切换目录或重启后恢复",
                    });
                    ui.add_space(6.0);
                }
            });

        // 底部栏改为叠加层，与地址栏一起在图片上层（先不占位，由后面的 CentralPanel 占满）
        // 堆叠区域（仅卡片；地址栏与底部栏在顶层 Area 叠加，不被图片遮挡）
        egui::CentralPanel::default()
            .frame(egui::Frame::none())
            .show(ctx, |ui| {
                let stack_rect = ui.available_rect_before_wrap();
                if stack_rect.is_positive() {
                    self.last_stack_size = stack_rect.size();
                    let screen_center = stack_rect.center();
                    // 画板模式：用物理屏幕坐标做 resize 补偿，避免拖左/上边时内容跟边跑（窗口相对坐标在拖左/上时可能不变）
                    let board_screen_center_for_compensation = if self.mode == AppMode::Board {
                        ui.ctx().input(|i| i.viewport().outer_rect.map(|outer| outer.min.to_vec2() + screen_center.to_vec2()))
                            .unwrap_or(screen_center.to_vec2())
                    } else {
                        screen_center.to_vec2()
                    };
                    let n = self.cards.len();
                    if n == 0 {
                        let msg = "当前文件夹中没有可以预览的图片";
                        let gray = egui::Color32::from_rgba_unmultiplied(90, 90, 90, 240);
                        let galley = ui.painter().layout_no_wrap(
                            msg.into(),
                            egui::FontId::proportional(15.0),
                            gray,
                        );
                        let padding = 14.0;
                        let frame_size = galley.size() + egui::Vec2::splat(2.0 * padding);
                        let frame_rect = egui::Rect::from_center_size(stack_rect.center(), frame_size);
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
                    if n > 0 && !self.initial_arrange_done {
                        self.initial_arrange_done = true;
                        self.stack_layout.available_size = stack_rect.size();
                        if self.mode != AppMode::Board && self.grid_view {
                            self.grid_arrange();
                        } else if self.mode == AppMode::Board && self.grid_view && self.layout_reference_size.is_none() {
                            // 画板模式且无已加载布局时也做首帧阵列，避免新目录下阵列不生效
                            self.grid_arrange();
                        }
                    }

                    // 画板模式专用：窗口 resize 时仅补偿 offset（取景框变化），不缩放画布内容；使用物理屏幕中心与窗口解耦
                    // 默认/灵感模式：同样补偿 view_offset，使原先在内容区中心的世界点仍留在原屏幕位置，避免拖边时所有图片移位
                    if self.mode == AppMode::Board {
                        board_view::apply_resize_compensation(
                            &mut self.board_view_offset,
                            self.board_view_scale,
                            board_screen_center_for_compensation,
                            self.last_board_screen_center,
                        );
                    } else {
                        let c = screen_center.to_vec2();
                        board_view::apply_resize_compensation(
                            &mut self.view_offset,
                            self.view_scale,
                            c,
                            self.last_stack_screen_center,
                        );
                    }

                    // 中键拖拽平移视角（默认/灵感用 view_offset；画板用 board_view 模块）
                    let middle_down = ui.input(|i| i.pointer.middle_down());
                    let pointer_delta = ui.input(|i| i.pointer.delta());
                    if middle_down && pointer_delta != egui::Vec2::ZERO {
                        let delta = if self.invert_pan { pointer_delta } else { -pointer_delta };
                        if self.mode == AppMode::Board {
                            board_view::apply_pan(&mut self.board_view_offset, delta);
                        } else {
                            self.view_offset += delta;
                        }
                    }
                    // 画板模式：空格+左键拖动 = 平移画布（复刻 PS）
                    if self.mode == AppMode::Board {
                        let space_held = ui.input(|i| i.key_down(egui::Key::Space));
                        if space_held && ui.input(|i| i.pointer.primary_down()) && pointer_delta != egui::Vec2::ZERO {
                            let delta = if self.invert_pan { pointer_delta } else { -pointer_delta };
                            board_view::apply_pan(&mut self.board_view_offset, delta);
                        }
                    }

                    let pointer = ui.input(|i| i.pointer.hover_pos());
                    let pointer_down = ui.input(|i| i.pointer.primary_down());
                    let pointer_released = ui.input(|i| i.pointer.primary_released());
                    let screen_rect = ui.ctx().screen_rect();
                    // 框选进行中：鼠标在窗口内时更新端点；移出窗口后保留上一帧端点，绘制与松手时夹在窗口边（参考 Windows 资源管理器）
                    if self.marquee_start_screen.is_some() {
                        if let Some(p) = pointer {
                            self.marquee_last_end_screen = Some(p);
                        }
                    }
                    let is_board = self.mode == AppMode::Board;
                    let (view_offset, view_scale) = if is_board {
                        (self.board_view_offset, self.board_view_scale)
                    } else {
                        (self.view_offset, self.view_scale)
                    };
                    let screen_center_v = screen_center.to_vec2();
                    let world_to_screen = |w: egui::Pos2| {
                        let p = if is_board {
                            board_view::canvas_to_screen(w.to_vec2(), screen_center_v, view_offset, view_scale)
                        } else {
                            let v = w.to_vec2() - view_offset;
                            egui::Vec2::new(screen_center_v.x + v.x * view_scale, screen_center_v.y + v.y * view_scale)
                        };
                        egui::Pos2::new(p.x, p.y)
                    };
                    let screen_to_world = |s: egui::Pos2| {
                        let p = if is_board {
                            board_view::screen_to_canvas(s.to_vec2(), screen_center_v, view_offset, view_scale)
                        } else {
                            let v = egui::Vec2::new(s.x - screen_center_v.x, s.y - screen_center_v.y) / view_scale;
                            view_offset + v
                        };
                        egui::Pos2::new(p.x, p.y)
                    };

                    // 未拿起任何图片时：滚轮缩放整个视图（画板用 board_view 以指针为中心缩放；默认/灵感保持原逻辑）
                    if self.picked_up.is_none() {
                        let scroll = ui.input(|i| i.raw_scroll_delta).y;
                        if scroll != 0.0 {
                            if self.mode == AppMode::Board {
                                if let Some(ptr) = pointer {
                                    board_view::apply_zoom_at_mouse(
                                        &mut self.board_view_offset,
                                        &mut self.board_view_scale,
                                        screen_center.to_vec2(),
                                        ptr.to_vec2(),
                                        scroll,
                                        VIEW_ZOOM_SENSITIVITY,
                                        VIEW_SCALE_MIN,
                                        VIEW_SCALE_MAX,
                                    );
                                }
                            } else {
                                let world_before = pointer.map(|p| screen_to_world(p).to_vec2());
                                self.view_scale = (self.view_scale * (1.0 + scroll * VIEW_ZOOM_SENSITIVITY))
                                    .clamp(VIEW_SCALE_MIN, VIEW_SCALE_MAX);
                                if let (Some(ptr), Some(wb)) = (pointer, world_before) {
                                    let s_center = screen_center.to_vec2();
                                    self.view_offset = wb - (ptr.to_vec2() - s_center) / self.view_scale;
                                }
                            }
                        }
                    }

                    // 命中检测（从最前到最后），使用世界/画布坐标
                    // 画板模式：画布固定，位置与缩放为绝对值，不随窗口 Resize 缩放（BOARD_MODE_OPTIMIZATION）；默认/灵感用 stack+random_offset
                    let picked_up_index = self.picked_up.as_ref().map(|(i, _, _)| *i);
                    // 画板模式：卡片显示尺寸用固定画布参考尺寸计算，与当前窗口解耦；无布局时用固定默认
                    let board_ref_size = self
                        .layout_reference_size
                        .unwrap_or(BOARD_DEFAULT_CANVAS_SIZE);
                    let card_display: Vec<(egui::Vec2, f32, f32)> = (0..n)
                        .map(|i| {
                            let card = &self.cards[i];
                        let (pos, scale_eff) = if self.mode == AppMode::Board {
                            // 画板：画布空间绝对值，不乘 board_scale_u；有保存则用 saved + anim，否则用堆叠+偏移（如阵列后或新目录）
                            card.board_saved_world_pos.map(|saved| {
                                    (saved + card.anim_position_delta, card.display_scale.clamp(DISPLAY_SCALE_MIN, DISPLAY_SCALE_MAX))
                                }).unwrap_or_else(|| {
                                    let (lp, _) = self.stack_layout.position_and_rotation(i, n);
                                    (lp.to_vec2() + card.random_offset + card.anim_position_delta, card.display_scale)
                                })
                            } else {
                                let (lp, _) = self.stack_layout.position_and_rotation(i, n);
                                (lp.to_vec2() + card.random_offset + card.anim_position_delta, card.display_scale)
                            };
                            let pos = if Some(i) == picked_up_index && self.held_smooth_pos.is_some() {
                                self.held_smooth_pos.unwrap_or(pos)
                            } else {
                                pos
                            };
                            let (_, rot) = self.stack_layout.position_and_rotation(i, n);
                            // 阵列状态下无随机旋转，一览状态下才有 layout_rot + random_rotation + 落定动画
                            let rot = if self.grid_view {
                                0.0
                            } else {
                                rot + card.random_rotation + card.anim_rotation_offset
                            };
                            let scale_eff = if Some(i) == picked_up_index { self.held_scale.unwrap_or(scale_eff) } else { scale_eff };
                            (pos, rot, scale_eff)
                        })
                        .collect();

                    let mut hovered_index = None;
                    let world_pointer = pointer.map(screen_to_world);
                    for i in (0..n).rev() {
                        if Some(i) == picked_up_index {
                            continue;
                        }
                        let (world_pos, rot, scale_eff) = card_display[i];
                        let card = &self.cards[i];
                        let size = if is_board {
                            crate::image_card::StackLayout::effective_display_size_in_area(
                                card.intrinsic_size, i, n, board_ref_size,
                            )
                        } else {
                            self.stack_layout.effective_display_size(card.intrinsic_size, i, n)
                        };
                        let size = size * scale_eff;
                        let half = size / 2.0;
                        if let Some(wp) = world_pointer {
                            if rotated_rect_contains(egui::Pos2::new(world_pos.x, world_pos.y), half, rot, wp) {
                                hovered_index = Some(i);
                                break;
                            }
                        }
                    }

                    // 更新每张卡的 anim_scale：仅悬停时稍微放大，不移动位置
                    // 仅在一览状态下对“刚放下”的那张卡做 anim_rotation_offset 向 0 收敛（落定动画）；阵列下无旋转
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
                    if let Some(idx) = dropped_idx {
                        if self.cards[idx].anim_rotation_offset.abs() < 1e-5 {
                            self.recently_dropped_index = None;
                        }
                    }

                    // 视口剔除：先用 R-Tree 查出与视口相交的卡（将计算量从 N 降到数十），再仅对查询结果做面积占比判断（可见 < 20% 才剔除）
                    const VISIBLE_RATIO_THRESHOLD: f64 = 0.2;
                    /// R-Tree 查询时视口向四周扩充的像素（大一圈的框，预加载邻域内的卡，减少平移时 pop-in）
                    const VIEWPORT_QUERY_PADDING: f32 = 500.0;
                    let viewport_for_culling = ui.clip_rect().expand2(egui::Vec2::splat(VIEWPORT_QUERY_PADDING));
                    let viewport_aabb = AABB::from_corners(
                        [viewport_for_culling.min.x as f64, viewport_for_culling.min.y as f64],
                        [viewport_for_culling.max.x as f64, viewport_for_culling.max.y as f64],
                    );
                    let mut rects: Vec<CardScreenRect> = Vec::with_capacity(n);
                    for (i, card) in self.cards.iter().enumerate() {
                        let (world_pos, rot, scale_eff) = card_display[i];
                        let world_center = egui::Pos2::new(world_pos.x, world_pos.y);
                        let size = if is_board {
                            crate::image_card::StackLayout::effective_display_size_in_area(
                                card.intrinsic_size, i, n, board_ref_size,
                            )
                        } else {
                            self.stack_layout.effective_display_size(card.intrinsic_size, i, n)
                        };
                        let size = size * scale_eff;
                        let half = size / 2.0;
                        let aabb = screen_aabb_for_rotated_rect(world_center, half, rot, &world_to_screen);
                        rects.push(CardScreenRect { index: i, aabb });
                    }
                    let tree = RTree::bulk_load(rects);
                    let visible_indices: HashSet<usize> = tree
                        .locate_in_envelope(&viewport_aabb)
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
                        .collect();

                    // 仅对可见卡（及拿起卡）请求纹理，堆叠前端优先
                    let paths_to_load: Vec<PathBuf> = self
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
                        let _ = self.ensure_texture(ctx, path);
                    }

                    // 绘制：仅绘制可见卡（可见比例 >= 20%，即 80% 在屏外才剔除），扩大裁剪区使图片可超出面板
                    let orig_clip = ui.clip_rect();
                    ui.set_clip_rect(ui.ctx().screen_rect().expand2(egui::Vec2::splat(5000.0)));
                    let painter = ui.painter();
                    let now = ctx.input(|i| i.time) as f32;
                    for (i, card) in self.cards.iter().enumerate() {
                        if self.picked_up.as_ref().map(|(idx, _, _)| *idx) == Some(i) {
                            continue;
                        }
                        if !visible_indices.contains(&i) {
                            continue;
                        }
                        let (world_pos, rot, scale_eff) = card_display[i];
                        let screen_pos = world_to_screen(egui::Pos2::new(world_pos.x, world_pos.y));
                        let size = if is_board {
                            crate::image_card::StackLayout::effective_display_size_in_area(
                                card.intrinsic_size, i, n, board_ref_size,
                            )
                        } else {
                            self.stack_layout.effective_display_size(card.intrinsic_size, i, n)
                        };
                        let size = size * scale_eff * view_scale;
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
                            // 阵列状态下拿起时也显示无旋转
                            let rot = if self.grid_view {
                                0.0
                            } else {
                                rot + card.random_rotation + card.anim_rotation_offset
                            };
                            let size = if is_board {
                                crate::image_card::StackLayout::effective_display_size_in_area(
                                    card.intrinsic_size, idx, n, board_ref_size,
                                )
                            } else {
                                self.stack_layout.effective_display_size(card.intrinsic_size, idx, n)
                            };
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

                    // 框选：选中卡片高亮描边；进行中的选框半透明填充+描边（选框可在整个窗口内绘制；鼠标移出窗口时端点夹在窗口边不消失，参考 Windows 资源管理器）
                    let marquee_draw_area = ui.ctx().screen_rect();
                    ui.set_clip_rect(marquee_draw_area);
                    let painter = ui.painter();
                    for &i in &self.selected_indices {
                        if i >= n {
                            continue;
                        }
                        let (world_pos, rot, scale_eff) = card_display[i];
                        let card = &self.cards[i];
                        let size = if is_board {
                            crate::image_card::StackLayout::effective_display_size_in_area(
                                card.intrinsic_size, i, n, board_ref_size,
                            )
                        } else {
                            self.stack_layout.effective_display_size(card.intrinsic_size, i, n)
                        };
                        let size = size * scale_eff;
                        let half = size / 2.0;
                        let world_center = egui::Pos2::new(world_pos.x, world_pos.y);
                        let aabb = screen_aabb_for_rotated_rect(world_center, half, rot, &world_to_screen);
                        let card_rect = egui::Rect::from_min_max(
                            egui::Pos2::new(aabb.lower()[0] as f32, aabb.lower()[1] as f32),
                            egui::Pos2::new(aabb.upper()[0] as f32, aabb.upper()[1] as f32),
                        );
                        painter.rect_stroke(
                            card_rect.expand(2.0),
                            egui::Rounding::same(0.0),
                            egui::Stroke::new(2.0, egui::Color32::from_rgb(60, 120, 220)),
                        );
                    }
                    // 当前端点：指针在窗口内用指针，移出窗口用上一帧端点并夹在窗口边（选框顶住边不消失）
                    let marquee_effective_cur = pointer
                        .or(self.marquee_last_end_screen)
                        .map(|p| egui::Pos2::new(
                            p.x.clamp(screen_rect.min.x, screen_rect.max.x),
                            p.y.clamp(screen_rect.min.y, screen_rect.max.y),
                        ));
                    if let (Some(start), Some(cur)) = (self.marquee_start_screen, marquee_effective_cur) {
                        let marquee_rect = egui::Rect::from_min_max(
                            egui::Pos2::new(start.x.min(cur.x), start.y.min(cur.y)),
                            egui::Pos2::new(start.x.max(cur.x), start.y.max(cur.y)),
                        );
                        let draw_rect = marquee_rect.intersect(marquee_draw_area);
                        if draw_rect.is_positive() {
                            painter.rect_filled(draw_rect, 0.0, egui::Color32::from_rgba_unmultiplied(60, 120, 220, 35));
                            painter.rect_stroke(draw_rect, egui::Rounding::same(0.0), egui::Stroke::new(1.5, egui::Color32::from_rgb(60, 120, 220)));
                        }
                    }
                    ui.set_clip_rect(orig_clip);

                    // 长按超时 → 进入“拿起”状态（卡片将跟手，松手时放下并更新位置/顺序；移动时可拖出窗口）
                    if self.picked_up.is_none() && self.long_press.is_some() {
                        if let Some((idx, start)) = self.long_press {
                            if ctx.input(|i| i.time) - start >= LONG_PRESS_DURATION as f64 {
                                let (world_pos, _, _) = card_display[idx];
                                let card_world = egui::Pos2::new(world_pos.x, world_pos.y);
                                let grab_offset_world = world_pointer
                                    .map(|wp| wp.to_vec2() - card_world.to_vec2())
                                    .unwrap_or(egui::Vec2::ZERO);
                                let now = ctx.input(|i| i.time);
                                self.picked_up = Some((idx, grab_offset_world, now));
                                self.held_smooth_pos = Some(card_world.to_vec2());
                                self.held_scale = Some(card_display[idx].2);
                                self.long_press = None;
                                if self.allow_drag_out {
                                    self.drag_pending = Some(idx); // 拿起后指针离开窗口边缘时发起系统拖出
                                }
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
                            let in_board = self.mode == AppMode::Board && self.layout_reference_size.is_some();
                            let (layout_front, new_layout_rot) = self.stack_layout.position_and_rotation(n.saturating_sub(1), n);
                            let (_, old_layout_rot) = self.stack_layout.position_and_rotation(idx, n);
                            let new_offset = drop_world - layout_front.to_vec2();
                            let current_total_rot = if let Some(card) = self.cards.get(idx) {
                                old_layout_rot + card.random_rotation + card.anim_rotation_offset
                            } else {
                                old_layout_rot - new_layout_rot
                            };
                            let old_world_pos: Vec<egui::Vec2> = (0..n)
                                .map(|i| {
                                    let (pos, _, _) = card_display[i];
                                    pos
                                })
                                .collect();
                            let old_layout_rots: Vec<f32> = (0..n)
                                .map(|i| self.stack_layout.position_and_rotation(i, n).1)
                                .collect();
                            let old_total_rots: Vec<f32> = (0..n)
                                .map(|i| old_layout_rots[i] + self.cards[i].random_rotation + self.cards[i].anim_rotation_offset)
                                .collect();
                            if let Some(card) = self.cards.get_mut(idx) {
                                // 放下时保留视觉尺寸：置顶后索引从 idx 变为 n-1，基准尺寸变大，需换算 display_scale 使 新基准×新scale = 旧基准×旧scale
                                let board_ref_size = self.layout_reference_size.unwrap_or(self.last_stack_size);
                                let old_effective = if in_board {
                                    crate::image_card::StackLayout::effective_display_size_in_area(
                                        card.intrinsic_size, idx, n, board_ref_size,
                                    )
                                } else {
                                    self.stack_layout.effective_display_size(card.intrinsic_size, idx, n)
                                };
                                let new_effective = if in_board {
                                    crate::image_card::StackLayout::effective_display_size_in_area(
                                        card.intrinsic_size, n - 1, n, board_ref_size,
                                    )
                                } else {
                                    self.stack_layout.effective_display_size(card.intrinsic_size, n - 1, n)
                                };
                                let new_scale = if new_effective.x > 1e-6 && new_effective.y > 1e-6 {
                                    let sx = scale * old_effective.x / new_effective.x;
                                    let sy = scale * old_effective.y / new_effective.y;
                                    sx.min(sy).clamp(DISPLAY_SCALE_MIN, DISPLAY_SCALE_MAX)
                                } else {
                                    scale.clamp(DISPLAY_SCALE_MIN, DISPLAY_SCALE_MAX)
                                };
                                if in_board {
                                    // 画板：画布空间绝对值，不随窗口缩放（BOARD_MODE_OPTIMIZATION）
                                    card.board_saved_world_pos = Some(drop_world);
                                    card.random_offset = drop_world;
                                    card.display_scale = new_scale;
                                } else {
                                    card.random_offset = new_offset;
                                    card.display_scale = new_scale;
                                }
                                // 阵列状态下无旋转、无落定动画；一览状态下保留随机旋转与落定
                                if self.grid_view {
                                    card.random_rotation = 0.0;
                                    card.anim_rotation_offset = 0.0;
                                } else {
                                    card.random_rotation = -new_layout_rot;
                                    card.anim_rotation_offset = normalize_angle_delta(current_total_rot);
                                }
                            }
                            self.bring_to_front(idx);
                            // 被置顶挤下去的卡片索引前移，基准尺寸变小，需按「旧索引→新索引」换算 display_scale 以保持视觉尺寸不变
                            let board_ref_size = self.layout_reference_size.unwrap_or(self.last_stack_size);
                            for i in idx..(n - 1) {
                                let (new_lp, new_layout_rot) = self.stack_layout.position_and_rotation(i, n);
                                let new_lp = new_lp.to_vec2();
                                if in_board {
                                    self.cards[i].board_saved_world_pos = Some(old_world_pos[i + 1]);
                                    self.cards[i].random_offset = old_world_pos[i + 1];
                                } else {
                                    self.cards[i].random_offset = old_world_pos[i + 1] - new_lp;
                                }
                                self.cards[i].anim_position_delta = egui::Vec2::ZERO;
                                // 阵列状态下不产生连带旋转，一览状态下才做旋转落定
                                if self.grid_view {
                                    self.cards[i].anim_rotation_offset = 0.0;
                                } else {
                                    self.cards[i].anim_rotation_offset = normalize_angle_delta(
                                        old_total_rots[i + 1] - new_layout_rot - self.cards[i].random_rotation,
                                    );
                                }
                                // 该卡由原索引 i+1 移到 i，基准变小，放大 display_scale 以保持视觉尺寸
                                let card = &mut self.cards[i];
                                let old_eff = if in_board {
                                    crate::image_card::StackLayout::effective_display_size_in_area(
                                        card.intrinsic_size, i + 1, n, board_ref_size,
                                    )
                                } else {
                                    self.stack_layout.effective_display_size(card.intrinsic_size, i + 1, n)
                                };
                                let new_eff = if in_board {
                                    crate::image_card::StackLayout::effective_display_size_in_area(
                                        card.intrinsic_size, i, n, board_ref_size,
                                    )
                                } else {
                                    self.stack_layout.effective_display_size(card.intrinsic_size, i, n)
                                };
                                if new_eff.x > 1e-6 && new_eff.y > 1e-6 {
                                    let ratio_x = old_eff.x / new_eff.x;
                                    let ratio_y = old_eff.y / new_eff.y;
                                    let ratio = ratio_x.min(ratio_y);
                                    card.display_scale = (card.display_scale * ratio).clamp(DISPLAY_SCALE_MIN, DISPLAY_SCALE_MAX);
                                }
                            }
                            self.recently_dropped_index = Some(n - 1);
                        }
                        self.long_press = None;
                        self.drag_pending = None;
                        // 框选结束或单击选择：若进行了框选则按选框与设置计算选中集（松手时若鼠标在窗外则用夹在窗口边的端点，与 Windows 一致）
                        if self.marquee_start_screen.is_some() {
                            let start = self.marquee_start_screen.take().unwrap();
                            let last_end = self.marquee_last_end_screen.take();
                            let effective_cur = pointer.or(last_end).map(|p| egui::Pos2::new(
                                p.x.clamp(screen_rect.min.x, screen_rect.max.x),
                                p.y.clamp(screen_rect.min.y, screen_rect.max.y),
                            ));
                            if let Some(cur) = effective_cur {
                                let marquee_rect = egui::Rect::from_min_max(
                                    egui::Pos2::new(start.x.min(cur.x), start.y.min(cur.y)),
                                    egui::Pos2::new(start.x.max(cur.x), start.y.max(cur.y)),
                                );
                                let left_to_right = cur.x >= start.x;
                                let use_contain = self.autodesk_style_selection && left_to_right;
                                let mut sel = HashSet::new();
                                for i in 0..n {
                                    let (world_pos, rot, scale_eff) = card_display[i];
                                    let card = &self.cards[i];
                                    let size = if is_board {
                                        crate::image_card::StackLayout::effective_display_size_in_area(
                                            card.intrinsic_size, i, n, board_ref_size,
                                        )
                                    } else {
                                        self.stack_layout.effective_display_size(card.intrinsic_size, i, n)
                                    };
                                    let size = size * scale_eff;
                                    let half = size / 2.0;
                                    let world_center = egui::Pos2::new(world_pos.x, world_pos.y);
                                    let aabb = screen_aabb_for_rotated_rect(world_center, half, rot, &world_to_screen);
                                    let card_rect = egui::Rect::from_min_max(
                                        egui::Pos2::new(aabb.lower()[0] as f32, aabb.lower()[1] as f32),
                                        egui::Pos2::new(aabb.upper()[0] as f32, aabb.upper()[1] as f32),
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
                                self.selected_indices = sel;
                            } else {
                                self.selected_indices.clear();
                            }
                        } else if let Some(idx) = hovered_index {
                            self.selected_indices.clear();
                            self.selected_indices.insert(idx);
                        } else {
                            self.selected_indices.clear();
                        }
                    } else if pointer_down {
                        if self.picked_up.is_none() {
                            if let Some(idx) = hovered_index {
                                if self.long_press.is_none() && self.marquee_start_screen.is_none() {
                                    let space_held = self.mode == AppMode::Board && ctx.input(|i| i.key_down(egui::Key::Space));
                                    if !space_held {
                                        self.long_press = Some((idx, ctx.input(|i| i.time)));
                                    }
                                }
                            } else {
                                self.long_press = None;
                                self.drag_pending = None;
                                let space_held = self.mode == AppMode::Board && ctx.input(|i| i.key_down(egui::Key::Space));
                                if !space_held && self.marquee_start_screen.is_none() {
                                    // 仅在尚未开始框选时设置起点；已在进行框选时不要用 pointer 覆盖（pointer 可能为 None 导致选框被清掉）
                                    self.marquee_start_screen = pointer;
                                    self.marquee_last_end_screen = pointer;
                                }
                            }
                        }
                    } else if hovered_index.is_none() && self.picked_up.is_none() {
                        self.long_press = None;
                        self.drag_pending = None;
                    }
                    if self.mode == AppMode::Board {
                        self.last_board_screen_center = Some(board_screen_center_for_compensation);
                    } else {
                        self.last_stack_screen_center = Some(screen_center.to_vec2());
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
                let a = (c.a() as f32 * 0.04).round() as u8;
                let bg = egui::Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a);
                ui.painter().rect_filled(
                    Rect::from_min_size(egui::Pos2::ZERO, ui.available_size()),
                    egui::Rounding::same(0.0),
                    bg,
                );
                ui.horizontal(|ui| {
                    if self.mode == AppMode::Default || self.mode == AppMode::Board {
                        let recent = self.recent_folders.clone();
                        let current_path = PathBuf::from(&self.watch_path);
                        egui::menu::menu_button(ui, "最近访问", |ui| {
                            ui.set_max_width(320.0);
                            ui.set_max_height(220.0);
                            ui.label(egui::RichText::new("最近到访").size(12.0).strong());
                            ui.separator();
                            if recent.is_empty() {
                                ui.label(egui::RichText::new("暂无").color(ui.visuals().weak_text_color()));
                            } else {
                                egui::ScrollArea::vertical().show(ui, |ui| {
                                    for path in &recent {
                                        if *path == current_path {
                                            continue;
                                        }
                                        let s = path.display().to_string();
                                        let truncate = if s.len() > 45 {
                                            let mut start = s.len().saturating_sub(44);
                                            while start < s.len() && !s.is_char_boundary(start) {
                                                start += 1;
                                            }
                                            format!("…{}", &s[start..])
                                        } else {
                                            s.clone()
                                        };
                                        if ui.button(truncate).on_hover_text(s).clicked() {
                                            self.navigate_watch_path(path.clone(), true);
                                            ui.close_menu();
                                        }
                                    }
                                });
                            }
                        })
                        .response
                        .on_hover_text("最近到访的 5 个文件夹，点击快速切换（有缓存）");
                        ui.separator();
                    }
                    if self.mode == AppMode::Default || self.mode == AppMode::Board {
                        if ui.input(|i| i.key_pressed(egui::Key::L) && (i.modifiers.ctrl || i.modifiers.mac_cmd)) {
                            ui.ctx().input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::L));
                            ui.ctx().input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::L));
                            self.address_bar_editing = true;
                            self.address_bar_edit_text = self.watch_path.clone();
                            self.address_bar_edit_request_focus = true;
                        }

                        let can_back = self.nav_index > 0;
                        let can_forward = self.nav_index < self.nav_history.len().saturating_sub(1);
                        if ui.add_enabled(can_back, egui::Button::new(" ◀ ").frame(false)).on_hover_text("后退").clicked() {
                            self.nav_back();
                        }
                        if ui.add_enabled(can_forward, egui::Button::new(" ▶ ").frame(false)).on_hover_text("前进").clicked() {
                            self.nav_forward();
                        }
                        ui.add_space(4.0);

                        if self.address_bar_editing {
                            let id = egui::Id::new("addr_bar_edit");
                            if self.address_bar_edit_request_focus {
                                ui.ctx().memory_mut(|m| m.request_focus(id));
                                self.address_bar_edit_request_focus = false;
                            }
                            let te = egui::TextEdit::singleline(&mut self.address_bar_edit_text)
                                .id(id)
                                .desired_width(ui.available_width().min(500.0))
                                .font(egui::TextStyle::Body);
                            let resp = ui.add(te);
                            if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                let s = self.address_bar_edit_text.trim();
                                if !s.is_empty() {
                                    let p = PathBuf::from(s);
                                    let path = std::fs::canonicalize(&p).unwrap_or(p);
                                    if path.is_dir() {
                                        self.navigate_watch_path(path, true);
                                    }
                                }
                                self.address_bar_editing = false;
                            }
                            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                self.address_bar_editing = false;
                            }
                            if resp.lost_focus() {
                                self.address_bar_editing = false;
                            }
                            let _ = resp;
                        } else {
                            let watch_path_buf = PathBuf::from(&self.watch_path);
                            let segments = path_breadcrumbs(&watch_path_buf);
                            let arrow_color = ui.visuals().weak_text_color();
                            let screen_w = ui.ctx().screen_rect().width();
                            const RESERVE_LEFT_RIGHT: f32 = 280.0;
                            let avail_breadcrumb = (screen_w - RESERVE_LEFT_RIGHT).max(40.0);
                            let (start_index, _seg_widths, _sep_w, _ellipsis_btn_w) =
                                breadcrumb_collapse_start(ui, &segments, avail_breadcrumb);

                            let _ = ui.allocate_ui_with_layout(
                                egui::Vec2::new(avail_breadcrumb, TOP_BAR_HEIGHT),
                                egui::Layout::left_to_right(egui::Align::Min),
                                |ui| {
                                    egui::ScrollArea::horizontal()
                                        .id_salt("addr_bar_breadcrumb")
                                        .max_height(TOP_BAR_HEIGHT - 4.0)
                                        .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        if start_index > 0 {
                                            let resp = egui::menu::menu_button(
                                                ui,
                                                egui::RichText::new("…").size(12.0).color(arrow_color),
                                                |ui| {
                                                    ui.set_max_width(280.0);
                                                    ui.set_max_height(200.0);
                                                    egui::ScrollArea::vertical().show(ui, |ui| {
                                                        for (path, label) in segments.iter().take(start_index) {
                                                            let label_str = label.to_string_lossy();
                                                            if ui.button(label_str.as_ref()).clicked() {
                                                                if path.as_os_str() != THIS_PC_SENTINEL
                                                                    && path.is_dir()
                                                                {
                                                                    self.navigate_watch_path(path.clone(), true);
                                                                }
                                                                ui.close_menu();
                                                            }
                                                        }
                                                    });
                                                },
                                            );
                                            if resp.response.hovered() {
                                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                            }
                                            resp.response.on_hover_text("被省略的上级路径，点击可跳转");
                                            ui.add(egui::Label::new(egui::RichText::new(" › ").color(arrow_color)).selectable(false));
                                        }
                                        for (i, (path, label)) in segments.iter().enumerate() {
                                            if i < start_index {
                                                continue;
                                            }
                                            let is_sentinel = path.as_os_str() == THIS_PC_SENTINEL;
                                            let is_current = i == segments.len() - 1;
                                            let label_str = label.to_string_lossy();
                                            let style = if is_current {
                                                egui::RichText::new(label_str.as_ref()).size(12.0).strong()
                                            } else {
                                                egui::RichText::new(label_str.as_ref()).size(12.0)
                                            };

                                            let seg_btn = egui::Button::new(style).frame(true).min_size(egui::Vec2::ZERO);
                                            let seg_resp = ui.add(seg_btn);
                                            if seg_resp.hovered() {
                                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                            }
                                            if seg_resp.clicked() {
                                                if is_sentinel {
                                                    #[cfg(windows)]
                                                    {
                                                        let drives = list_drives_windows();
                                                        if let Some(first) = drives.first() {
                                                            self.navigate_watch_path(first.clone(), true);
                                                        }
                                                    }
                                                } else if path.is_dir() {
                                                    self.navigate_watch_path(path.clone(), true);
                                                }
                                            }
                                            seg_resp.on_hover_text(if is_current { "当前监听目录" } else { "点击切换到该目录" });

                                            if is_sentinel {
                                                #[cfg(windows)]
                                                {
                                                    let drives = list_drives_windows();
                                                    egui::menu::menu_button(ui, egui::RichText::new(" › ").color(arrow_color), |ui| {
                                                        ui.set_max_width(200.0);
                                                        ui.set_max_height(200.0);
                                                        egui::ScrollArea::vertical().show(ui, |ui| {
                                                            for drive in &drives {
                                                                let name = drive.display().to_string();
                                                                if ui.button(name.as_str()).clicked() {
                                                                    self.navigate_watch_path(drive.clone(), true);
                                                                    ui.close_menu();
                                                                }
                                                            }
                                                        });
                                                    });
                                                }
                                                #[cfg(not(windows))]
                                                {
                                                    ui.add(egui::Label::new(egui::RichText::new(" › ").color(arrow_color)).selectable(false));
                                                }
                                            } else {
                                                let subdirs = list_subdirs(path, 80);
                                                if !subdirs.is_empty() {
                                                    egui::menu::menu_button(ui, egui::RichText::new(" › ").color(arrow_color), |ui| {
                                                        ui.set_max_width(220.0);
                                                        ui.set_max_height(200.0);
                                                        egui::ScrollArea::vertical().show(ui, |ui| {
                                                            for subdir in &subdirs {
                                                                let name = subdir.file_name().unwrap_or_default().to_string_lossy();
                                                                if ui.button(name.as_ref()).clicked() {
                                                                    self.navigate_watch_path(subdir.clone(), true);
                                                                    ui.close_menu();
                                                                }
                                                            }
                                                        });
                                                    });
                                                } else {
                                                    ui.add(egui::Label::new(egui::RichText::new(" › ").color(arrow_color)).selectable(false));
                                                }
                                            }
                                        }
                                    });
                                });
                            }
                            );

                            let blank_id = egui::Id::new("addr_blank");
                            let avail = ui.available_width();
                            if avail > 4.0 {
                                let blank_resp = ui.allocate_ui(egui::Vec2::new(avail, TOP_BAR_HEIGHT), |ui| {
                                    let rect = ui.available_rect_before_wrap();
                                    ui.interact(rect, blank_id, egui::Sense::click())
                                });
                                if blank_resp.inner.hovered() {
                                    ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
                                }
                                if blank_resp.inner.clicked() {
                                    self.address_bar_editing = true;
                                    self.address_bar_edit_text = self.watch_path.clone();
                                    self.address_bar_edit_request_focus = true;
                                }
                                blank_resp.response.on_hover_text("点击或 Ctrl+L 输入/粘贴路径");
                            }

                        }
                    } else {
                        ui.label(format!("监听: {}", self.watch_path))
                            .on_hover_text("从 Chrome 拖入无效时，可复制图片后 Ctrl+V（Mac：Cmd+V）粘贴保存");
                    }
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
                        self.texture_cache_scope.clear();
                        self.pending_texture_paths.clear();
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
                let a = (c.a() as f32 * 0.04).round() as u8;
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
                            .selectable_label(self.grid_view, egui::RichText::new("阵列").size(13.0))
                            .on_hover_text("按网格、原比例排列，快速观看全貌；之后仍可自由拖动")
                            .clicked()
                        {
                            self.stack_layout.available_size = self.last_stack_size;
                            self.grid_arrange();
                        }
                        if ui
                            .selectable_label(!self.grid_view, egui::RichText::new("一览").size(13.0))
                            .on_hover_text("将图片打散并均匀分布，快速浏览文件夹内容")
                            .clicked()
                        {
                            self.stack_layout.available_size = self.last_stack_size;
                            self.scatter_spread();
                        }
                        // 右侧：左键点住某张图时显示该图完整文件名，以及始终显示版本号（半透明灰色）
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                let gray = egui::Color32::from_rgba_unmultiplied(100, 100, 100, 160);
                                let version = env!("CARGO_PKG_VERSION");
                                ui.label(
                                    egui::RichText::new(format!("v{}", version))
                                        .color(gray)
                                        .size(11.0),
                                );
                                let held_index = self
                                    .long_press
                                    .as_ref()
                                    .map(|(i, _)| *i)
                                    .or_else(|| self.picked_up.as_ref().map(|(i, _, _)| *i));
                                if let Some(idx) = held_index {
                                    if let Some(card) = self.cards.get(idx) {
                                        let name = card.path
                                            .file_name()
                                            .and_then(|n| n.to_str())
                                            .unwrap_or(card.file_name.as_str());
                                        ui.add_space(8.0);
                                        ui.add(
                                            egui::Label::new(
                                                egui::RichText::new(name)
                                                    .color(gray)
                                                    .size(11.0),
                                            )
                                            .truncate(),
                                        );
                                    }
                                }
                            },
                        );
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

        // 仅在需要连续重绘时请求 repaint，避免每帧强制重绘导致透明窗口抖动。
        // 包含缩放/平移：画板用 board_view_*，默认/灵感用 view_*，一览与阵列会重置对应 view。
        let view_needs_repaint = if self.mode == AppMode::Board {
            (self.board_view_scale - 1.0).abs() > 1e-6 || self.board_view_offset.length_sq() > 1e-6
        } else {
            (self.view_scale - 1.0).abs() > 1e-6 || self.view_offset.length_sq() > 1e-6
        };
        let needs_continuous_repaint = self.picked_up.is_some()
            || view_needs_repaint
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
