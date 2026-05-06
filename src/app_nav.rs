//! 地址栏面包屑与盘符列表：纯路径/UI 宽度计算，与 `DeckRefApp` 解耦。

use eframe::egui;
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

/// 面包屑根「此台电脑」的占位路径，不参与真实导航，仅用于展开盘符列表（Windows）
pub(crate) const THIS_PC_SENTINEL: &str = "::THIS_PC::";

/// 面包屑单段按钮水平内边距（用于宽度估算）
const BREADCRUMB_BTN_PADDING_X: f32 = 16.0;

/// Windows：将用户或 UI 可能传入的 `F:` 规范化为 `F:\`，避免 `is_dir` / 监听落在「该盘当前工作目录」而非盘根。
#[cfg(windows)]
pub(crate) fn normalize_watch_path_for_nav(path: PathBuf) -> PathBuf {
    use std::path::MAIN_SEPARATOR;
    let s = path.to_string_lossy();
    let b = s.as_bytes();
    if b.len() == 2 && b[1] == b':' && b[0].is_ascii_alphabetic() {
        return PathBuf::from(format!("{}{}", s, MAIN_SEPARATOR));
    }
    path
}

#[cfg(not(windows))]
pub(crate) fn normalize_watch_path_for_nav(path: PathBuf) -> PathBuf {
    path
}

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
///
/// **Windows 注意**：`F:`（无 `\`）在系统语义下表示「F 盘当前工作目录」，不是盘根。必须在盘符后合并
/// [`Component::RootDir`] 再写入首段，否则用户点「F:」会跳到进程在该盘上的当前目录（例如某项目的 `deckref`）。
pub(crate) fn path_breadcrumbs(path: &Path) -> Vec<(PathBuf, OsString)> {
    let mut out = Vec::new();
    let mut acc = PathBuf::new();
    let mut it = path.components().peekable();
    while let Some(comp) = it.next() {
        match comp {
            Component::Prefix(prefix) => {
                acc.push(comp);
                // Windows：盘符后紧跟根 `\` 时一并累积，再输出，避免 PathBuf 仅为 `F:`。
                if let Some(&Component::RootDir) = it.peek() {
                    acc.push(it.next().expect("peeked RootDir"));
                    out.push((acc.clone(), prefix.as_os_str().to_owned()));
                } else {
                    out.push((acc.clone(), prefix.as_os_str().to_owned()));
                }
            }
            Component::RootDir => {
                acc.push(comp);
                // Unix 等：无前缀时单独出现根目录
                if out.is_empty() {
                    out.push((acc.clone(), OsString::from(std::path::MAIN_SEPARATOR.to_string())));
                }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn breadcrumb_drive_segment_is_root_not_drive_relative() {
        let segs = path_breadcrumbs(Path::new(r"F:\foo\deckref"));
        assert!(segs.len() >= 3, "{:?}", segs);
        assert_eq!(segs[0].0, PathBuf::from(THIS_PC_SENTINEL));
        let (drive_path, drive_label) = &segs[1];
        assert_eq!(drive_label.to_string_lossy(), "F:");
        assert!(
            drive_path.ends_with("\\") || drive_path.to_string_lossy().ends_with('/'),
            "expected drive root path, got {:?}",
            drive_path
        );
    }

    #[cfg(windows)]
    #[test]
    fn normalize_watch_path_for_nav_fixes_drive_letter_only() {
        let p = PathBuf::from("G:");
        let n = normalize_watch_path_for_nav(p);
        assert!(n.ends_with("\\"), "{:?}", n);
    }
}
