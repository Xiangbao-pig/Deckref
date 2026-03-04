//! Windows「最近使用的文件」中的图片路径（%APPDATA%\\Microsoft\\Windows\\Recent 内的 .lnk 解析）。
//! 仅当 cfg(windows) 时编译。

use crate::image_ops;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// 灵感模式下「最近图片」张数上限。
pub const INSPIRATION_RECENT_MAX: usize = 30;

/// 返回 Windows 最近使用的文件中的图片路径，按最近使用顺序，最多 `max` 张。
/// `extended` 为真时包含专业格式扩展名（TGA、ICO、DDS 等）。
pub fn list_recent_image_paths(max: usize, extended: bool) -> Vec<PathBuf> {
    let recent_dir = match std::env::var("APPDATA") {
        Ok(appdata) => PathBuf::from(appdata).join("Microsoft").join("Windows").join("Recent"),
        Err(_) => return Vec::new(),
    };
    let mut entries: Vec<_> = match std::fs::read_dir(&recent_dir) {
        Ok(rd) => rd.filter_map(|e| e.ok()).collect(),
        Err(_) => return Vec::new(),
    };
    // 按 .lnk 修改时间新→旧
    entries.sort_by(|a, b| {
        let ta = a.metadata().and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
        let tb = b.metadata().and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
        tb.cmp(&ta)
    });
    let mut out = Vec::with_capacity(max);
    for entry in entries {
        if out.len() >= max {
            break;
        }
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("lnk") {
            continue;
        }
        let target = resolve_lnk_target(&path);
        let target = match target {
            Some(t) => t,
            None => continue,
        };
        let target = PathBuf::from(target);
        if !target.is_file() || !image_ops::is_image_path_with(&target, extended) {
            continue;
        }
        out.push(target);
    }
    out
}

/// 解析 .lnk 文件，返回目标路径（本地路径字符串）。失败或网络路径时返回 None。
fn resolve_lnk_target(lnk_path: &Path) -> Option<String> {
    use lnk::ShellLink;
    let link = ShellLink::open(lnk_path, lnk::encoding::WINDOWS_1252).ok()?;
    let target = link.link_target()?;
    if target.starts_with("\\\\") {
        return None;
    }
    Some(target)
}
