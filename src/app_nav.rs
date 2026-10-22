//! 地址栏面包屑与盘符列表：纯路径/UI 宽度计算，与 `DeckRefApp` 解耦。

use eframe::egui;
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

/// 面包屑根「此电脑」的占位路径。它不是真实目录，只用来展开和资源管理器一样的「此电脑」列表。
pub(crate) const THIS_PC_SENTINEL: &str = "::THIS_PC::";

pub(crate) const THIS_PC_LABEL: &str = "此电脑";

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
        out.insert(0, (PathBuf::from(THIS_PC_SENTINEL), OsString::from(THIS_PC_LABEL)));
    }
    out
}

/// 资源管理器里的显示名。盘符会变成「本地磁盘 (C:)」，特殊文件夹会变成「图片」这类名称。
pub(crate) fn shell_display_name(path: &Path) -> String {
    #[cfg(windows)]
    {
        if let Some(name) = windows_display_name(path) {
            if !name.is_empty() {
                return name;
            }
        }
    }
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| path.display().to_string())
}

/// 面包屑上这一段的文字。根是「此电脑」，其余用资源管理器显示名。
pub(crate) fn breadcrumb_label(path: &Path, fallback: &std::ffi::OsStr) -> String {
    if path.as_os_str() == THIS_PC_SENTINEL {
        return THIS_PC_LABEL.to_string();
    }
    let name = shell_display_name(path);
    if name.is_empty() {
        fallback.to_string_lossy().into_owned()
    } else {
        name
    }
}

/// 「此电脑」箭头下的列表：先是桌面/文档/下载/音乐/图片/视频，再是各盘符。顺序与 Windows 11 地址栏一致。
pub(crate) fn this_pc_entries() -> Vec<(PathBuf, String)> {
    #[cfg(windows)]
    {
        let mut out = Vec::new();
        for folder in known_this_pc_folders() {
            if folder.is_dir() {
                let label = shell_display_name(&folder);
                out.push((folder, label));
            }
        }
        let mut drives = list_drives_windows();
        drives.sort_by(|a, b| logical_cmp(&shell_display_name(a), &shell_display_name(b)));
        for drive in drives {
            let label = shell_display_name(&drive);
            out.push((drive, label));
        }
        out
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

/// 某一级箭头下的子文件夹。只含目录，隐藏和系统目录不出现，按资源管理器的逻辑顺序排列。
pub(crate) fn list_child_folders(path: &Path) -> Vec<(PathBuf, String)> {
    let mut entries: Vec<(PathBuf, String)> = match std::fs::read_dir(path) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir() && !is_hidden_or_system(&e.path()))
            .map(|e| {
                let path = e.path();
                let label = shell_display_name(&path);
                (path, label)
            })
            .collect(),
        Err(_) => return Vec::new(),
    };
    entries.sort_by(|a, b| logical_cmp(&a.1, &b.1));
    entries
}

/// 空间不够时从左侧收起。Windows 11 保留右侧各级，左侧只留一个箭头；点它列出被收起的上级。
/// 每一级右边都有自己的箭头，所以宽度按「名称 + 箭头」计算。
/// 返回 (start_index, 箭头按钮宽度)。start_index > 0 时只显示 labels[start_index..]。
pub(crate) fn breadcrumb_collapse_start(
    ui: &egui::Ui,
    labels: &[String],
    available_width: f32,
) -> (usize, f32) {
    let font_id = egui::FontId::proportional(12.0);
    let color = egui::Color32::PLACEHOLDER;
    let chevron_galley = ui.fonts(|f| f.layout_no_wrap("›".to_string(), font_id.clone(), color));
    let chevron_w = chevron_galley.size().x + 14.0;

    let mut seg_widths = Vec::with_capacity(labels.len());
    for label in labels {
        let galley = ui.fonts(|f| f.layout_no_wrap(label.clone(), font_id.clone(), color));
        seg_widths.push(galley.size().x + BREADCRUMB_BTN_PADDING_X);
    }

    let n = labels.len();
    if n == 0 {
        return (0, chevron_w);
    }

    let full_width: f32 = seg_widths.iter().sum::<f32>() + n as f32 * chevron_w;
    let avail = (available_width - 4.0).max(20.0);
    if full_width <= avail {
        return (0, chevron_w);
    }

    let mut start_index = n - 1;
    for start in 0..n {
        let mut w = chevron_w;
        for width in &seg_widths[start..] {
            w += width + chevron_w;
        }
        if w <= avail {
            start_index = start;
            break;
        }
    }
    (start_index, chevron_w)
}

fn logical_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::core::PCWSTR;
        use windows::Win32::UI::Shell::StrCmpLogicalW;
        let aw: Vec<u16> = std::ffi::OsStr::new(a).encode_wide().chain(Some(0)).collect();
        let bw: Vec<u16> = std::ffi::OsStr::new(b).encode_wide().chain(Some(0)).collect();
        let ord = unsafe { StrCmpLogicalW(PCWSTR(aw.as_ptr()), PCWSTR(bw.as_ptr())) };
        ord.cmp(&0)
    }
    #[cfg(not(windows))]
    {
        a.to_lowercase().cmp(&b.to_lowercase())
    }
}

fn is_hidden_or_system(path: &Path) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const HIDDEN: u32 = 0x2;
        const SYSTEM: u32 = 0x4;
        if let Ok(meta) = std::fs::metadata(path) {
            return meta.file_attributes() & (HIDDEN | SYSTEM) != 0;
        }
    }
    let _ = path;
    false
}

#[cfg(windows)]
fn windows_display_name(path: &Path) -> Option<String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
    use windows::Win32::UI::Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_DISPLAYNAME};
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut info = SHFILEINFOW::default();
    let ok = unsafe {
        SHGetFileInfoW(
            PCWSTR(wide.as_ptr()),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(std::ptr::addr_of_mut!(info)),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_DISPLAYNAME,
        )
    };
    if ok == 0 {
        return None;
    }
    let len = info.szDisplayName.iter().position(|&c| c == 0).unwrap_or(info.szDisplayName.len());
    let name = String::from_utf16_lossy(&info.szDisplayName[..len]);
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

#[cfg(windows)]
fn known_this_pc_folders() -> Vec<PathBuf> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{
        SHGetKnownFolderPath, FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_Music,
        FOLDERID_Pictures, FOLDERID_Videos, KF_FLAG_DEFAULT,
    };
    let ids = [
        &FOLDERID_Desktop,
        &FOLDERID_Documents,
        &FOLDERID_Downloads,
        &FOLDERID_Music,
        &FOLDERID_Pictures,
        &FOLDERID_Videos,
    ];
    let mut out = Vec::new();
    for id in ids {
        let Ok(pw) = (unsafe { SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None) }) else {
            continue;
        };
        let text = unsafe { pw.to_string() }.unwrap_or_default();
        unsafe { CoTaskMemFree(Some(pw.0 as *const _)) };
        if !text.is_empty() {
            out.push(PathBuf::from(text));
        }
    }
    out
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
