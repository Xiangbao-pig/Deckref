//! 窗口状态持久化：记录上次关闭时的窗口尺寸、位置，下次启动时恢复。
//! 面向易用性：用户调整好的窗口大小与所在显示器（通过位置体现）不会在重启后丢失。
//! 同时提供全局最小尺寸限制，避免窗口被缩得过小无法操作。

use crate::error::{AppError, AppResult};
use std::path::{Path, PathBuf};

/// 默认窗口内宽（与 main.rs ViewportBuilder 一致）
const DEFAULT_WIDTH: f64 = 360.0;
/// 默认窗口内高
const DEFAULT_HEIGHT: f64 = 240.0;
/// 恢复时宽度下限，避免无效或过小
const MIN_WIDTH: f64 = 200.0;
/// 恢复时宽度上限（约 4K 宽）
const MAX_WIDTH: f64 = 3840.0;
/// 恢复时高度下限
const MIN_HEIGHT: f64 = 150.0;
/// 恢复时高度上限（约 4K 高）
const MAX_HEIGHT: f64 = 2160.0;

/// 应用级最小内尺寸：窗口不允许被用户拖到比这更小（ViewportBuilder::with_min_inner_size）。
pub const MIN_INNER_WIDTH: f32 = 200.0;
/// 应用级最小内尺寸高度
pub const MIN_INNER_HEIGHT: f32 = 150.0;

/// 恢复时位置坐标下限（避免明显 off-screen）
const POS_MIN: f64 = -16000.0;
/// 恢复时位置坐标上限
const POS_MAX: f64 = 16000.0;

/// 从磁盘加载的完整窗口状态（位置 + 尺寸）。
#[derive(Clone, Debug)]
pub struct WindowState {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl WindowState {
    fn clamp(self) -> Self {
        Self {
            x: self.x.clamp(POS_MIN, POS_MAX),
            y: self.y.clamp(POS_MIN, POS_MAX),
            width: self.width.clamp(MIN_WIDTH, MAX_WIDTH),
            height: self.height.clamp(MIN_HEIGHT, MAX_HEIGHT),
        }
    }
}

/// 返回窗口状态文件路径。Windows: %APPDATA%\\DeckRef\\window.txt，Unix: ~/.config/deckref/window.txt。
pub fn window_state_path() -> AppResult<PathBuf> {
    Ok(config_dir()?.join("window.txt"))
}

/// 返回配置目录（与 window_state 共用）。Windows: %APPDATA%\\DeckRef，Unix: ~/.config/deckref。
pub fn config_dir() -> AppResult<PathBuf> {
    #[cfg(windows)]
    let dir = {
        let appdata = std::env::var("APPDATA").or_else(|_| std::env::var("USERPROFILE"));
        let base = appdata.map_err(AppError::EnvVar)?;
        PathBuf::from(base).join("DeckRef")
    };
    #[cfg(not(windows))]
    let dir = {
        let home = std::env::var("HOME").map_err(AppError::EnvVar)?;
        let config = std::env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(home).join(".config"));
        config.join("deckref")
    };
    Ok(dir)
}

/// 从磁盘读取上次保存的窗口状态（位置 + 尺寸）；无文件或解析失败时返回 None。
/// 位置用于恢复窗口所在显示器（系统会根据位置将窗口放到包含该点的显示器上）。
pub fn load_window_state() -> Option<WindowState> {
    let path = window_state_path().ok()?;
    let s = std::fs::read_to_string(&path).ok()?;
    let mut lines = s.lines().filter(|l| !l.trim().is_empty());
    let x: f64 = lines.next()?.trim().parse().ok()?;
    let y: f64 = lines.next()?.trim().parse().ok()?;
    let w: f64 = lines.next()?.trim().parse().ok()?;
    let h: f64 = lines.next()?.trim().parse().ok()?;
    Some(WindowState { x, y, width: w, height: h }.clamp())
}

/// 兼容旧版两行格式（仅宽高），返回 None 表示使用默认（无保存位置）。
fn load_window_size_legacy() -> Option<(f64, f64)> {
    let path = window_state_path().ok()?;
    let s = std::fs::read_to_string(&path).ok()?;
    let mut lines = s.lines().filter(|l| !l.trim().is_empty());
    let w: f64 = lines.next()?.trim().parse().ok()?;
    let h: f64 = lines.next()?.trim().parse().ok()?;
    let w = w.clamp(MIN_WIDTH, MAX_WIDTH);
    let h = h.clamp(MIN_HEIGHT, MAX_HEIGHT);
    Some((w, h))
}

/// 将当前窗口外框位置与内尺寸写入磁盘。会创建父目录（如不存在）。
/// `outer_pos`: 窗口外框左上角 (x, y)（含标题栏）；`inner_size`: 内容区宽高。
pub fn save_window_state(outer_pos_x: f64, outer_pos_y: f64, inner_width: f64, inner_height: f64) -> AppResult<()> {
    let path = window_state_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(AppError::Io)?;
    }
    let x = outer_pos_x.clamp(POS_MIN, POS_MAX);
    let y = outer_pos_y.clamp(POS_MIN, POS_MAX);
    let w = inner_width.clamp(MIN_WIDTH, MAX_WIDTH);
    let h = inner_height.clamp(MIN_HEIGHT, MAX_HEIGHT);
    let content = format!("{}\n{}\n{}\n{}\n", x, y, w, h);
    std::fs::write(&path, content).map_err(AppError::Io)?;
    Ok(())
}

/// 返回用于 ViewportBuilder 的初始内尺寸；若存在已保存状态则用其尺寸（已做边界约束）。返回 f32 以匹配 egui::Vec2。
pub fn initial_inner_size() -> [f32; 2] {
    if let Some(s) = load_window_state() {
        return [s.width as f32, s.height as f32];
    }
    if let Some((w, h)) = load_window_size_legacy() {
        return [w as f32, h as f32];
    }
    [DEFAULT_WIDTH as f32, DEFAULT_HEIGHT as f32]
}

/// 返回用于 ViewportBuilder 的初始外框位置；若存在已保存状态则用其位置（已做边界约束）。返回 f32 以匹配 egui::Pos2。
/// 无保存位置时返回 None，不设置 with_position（由系统决定）。
pub fn initial_position() -> Option<[f32; 2]> {
    load_window_state().map(|s| [s.x as f32, s.y as f32])
}

/// 默认模式「上次监听目录」的存储文件名（与 window.txt 同目录）。
fn last_default_path_file() -> AppResult<PathBuf> {
    Ok(config_dir()?.join("last_path.txt"))
}

/// 读取上次默认模式监听的目录路径；无文件或路径无效时返回 None。
pub fn load_last_default_path() -> Option<PathBuf> {
    let path = last_default_path_file().ok()?;
    let s = std::fs::read_to_string(&path).ok()?;
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let p = PathBuf::from(s);
    if p.is_dir() {
        Some(p)
    } else {
        None
    }
}

/// 保存默认模式当前监听目录，供下次启动恢复。
pub fn save_last_default_path(path: &Path) -> AppResult<()> {
    let file = last_default_path_file()?;
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(AppError::Io)?;
    }
    let s = path.display().to_string();
    std::fs::write(&file, s).map_err(AppError::Io)?;
    Ok(())
}

/// 最近到访文件夹列表的存储文件名（与 window.txt 同目录），每行一个路径。
fn recent_folders_file() -> AppResult<PathBuf> {
    Ok(config_dir()?.join("recent_folders.txt"))
}

/// 从磁盘读取最近到访的文件夹路径（最多 5 个）；只返回仍存在且为目录的路径。
pub fn load_recent_folders(max: usize) -> Vec<PathBuf> {
    let path = match recent_folders_file() {
        Ok(p) => p,
        Err(_) => return vec![],
    };
    let s = match std::fs::read_to_string(&path) {
        Ok(x) => x,
        Err(_) => return vec![],
    };
    s.lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .take(max)
        .collect()
}

/// 将最近到访的文件夹路径列表写入磁盘（每行一个路径）。
pub fn save_recent_folders(paths: &[PathBuf]) -> AppResult<()> {
    let file = recent_folders_file()?;
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(AppError::Io)?;
    }
    let content: String = paths
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&file, content).map_err(AppError::Io)?;
    Ok(())
}

/// 应用级最小内尺寸，用于 ViewportBuilder::with_min_inner_size。
pub fn min_inner_size() -> [f32; 2] {
    [MIN_INNER_WIDTH, MIN_INNER_HEIGHT]
}
