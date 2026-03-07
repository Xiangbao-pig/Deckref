//! 应用设置持久化：与窗口状态共用配置目录，关闭再打开后恢复。
//! 格式轻量：纯文本、按行解析，无额外依赖。

use crate::error::{AppError, AppResult};
use crate::window_state;
use std::path::PathBuf;

/// 设置项：与设置窗口中的选项一一对应。
#[derive(Clone, Debug)]
pub struct AppSettings {
    /// 反转中键平移方向
    pub invert_pan: bool,
    /// 拿起图片后拖出窗口边缘时是否发起系统文件拖放（默认开启）
    pub allow_drag_out: bool,
    /// 预览专业格式图片（TGA、ICO、DDS、AVIF、EXR、HDR、PNM、QOI、PSD 等）
    pub preview_professional_formats: bool,
    /// 框选方式：开 = AutoDesk 式（左→右 仅完全落入选框的选中，右→左 沾到即选中）；关 = 统一交叉选择（沾到即选中）
    pub autodesk_style_selection: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            invert_pan: false,
            allow_drag_out: true,
            preview_professional_formats: false,
            autodesk_style_selection: false,
        }
    }
}

fn settings_path() -> AppResult<PathBuf> {
    Ok(window_state::config_dir()?.join("settings.txt"))
}

/// 从磁盘加载设置；无文件或解析失败时返回默认值。
pub fn load_settings() -> AppSettings {
    let path = match settings_path() {
        Ok(p) => p,
        Err(_) => return AppSettings::default(),
    };
    let s = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return AppSettings::default(),
    };
    let mut lines = s.lines().map(|l| l.trim()).filter(|l| !l.is_empty());
    let invert_pan = lines
        .next()
        .and_then(|l| l.parse::<u8>().ok())
        .map(|n| n != 0)
        .unwrap_or(false);
    let allow_drag_out = lines
        .next()
        .and_then(|l| l.parse::<u8>().ok())
        .map(|n| n != 0)
        .unwrap_or(true);
    let preview_professional_formats = lines
        .next()
        .and_then(|l| l.parse::<u8>().ok())
        .map(|n| n != 0)
        .unwrap_or(false);
    let autodesk_style_selection = lines
        .next()
        .and_then(|l| l.parse::<u8>().ok())
        .map(|n| n != 0)
        .unwrap_or(false);
    AppSettings {
        invert_pan,
        allow_drag_out,
        preview_professional_formats,
        autodesk_style_selection,
    }
}

/// 将当前设置写入磁盘；会创建配置目录（若不存在）。
pub fn save_settings(s: &AppSettings) -> AppResult<()> {
    let path = settings_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(AppError::Io)?;
    }
    let content = format!(
        "{}\n{}\n{}\n{}\n",
        if s.invert_pan { 1 } else { 0 },
        if s.allow_drag_out { 1 } else { 0 },
        if s.preview_professional_formats { 1 } else { 0 },
        if s.autodesk_style_selection { 1 } else { 0 }
    );
    std::fs::write(&path, content).map_err(AppError::Io)?;
    Ok(())
}
