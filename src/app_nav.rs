//! 地址栏面包屑与盘符列表：纯路径/UI 宽度计算，与 `DeckRefApp` 解耦。

use eframe::egui;
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

/// 面包屑根「此台电脑」的占位路径，不参与真实导航，仅用于展开盘符列表（Windows）
pub(crate) const THIS_PC_SENTINEL: &str = "::THIS_PC::";

/// 面包屑单段按钮水平内边距（用于宽度估算）
const BREADCRUMB_BTN_PADDING_X: f32 = 16.0;

/// Windows：列出所有逻辑盘符路径（如 C:\、D:\）。
#[cfg(windows)]
pub(crate) fn list_drives_windows() -> Vec<PathBuf> {
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
pub(crate) fn path_breadcrumbs(path: &Path) -> Vec<(PathBuf, OsString)> {
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
pub(crate) fn list_subdirs(path: &Path, limit: usize) -> Vec<PathBuf> {
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

/// 计算面包屑在空间不足时从左侧折叠的起始索引（参考 Win11 资源管理器：保留右侧，左侧用省略号）。
/// 当窗口足够长、可用宽度能容纳完整路径时，自动展开显示全部段落（start_index = 0）。
/// 返回 (start_index, 各段宽度, 分隔符宽度, 省略号按钮宽度)。start_index > 0 时表示需显示 "…" 并只显示 segments[start_index..]。
pub(crate) fn breadcrumb_collapse_start(
    ui: &egui::Ui,
    segments: &[(PathBuf, OsString)],
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
