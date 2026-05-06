//! 后台加载与纹理解码线程入口（与 UI 主线程通过 channel 通信）。

use crate::image_ops;
use crate::mode::AppMode;
use crate::watcher::list_existing_image_paths_with_metadata;
#[cfg(not(windows))]
use crate::watcher::list_existing_image_paths;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::SystemTime;

pub(crate) fn run_loader_thread(
    mode: AppMode,
    watch_path: PathBuf,
    screenshot_dir: PathBuf,
    extended_formats: bool,
    max_cards: usize,
    inspiration_recent_max: usize,
    tx: mpsc::Sender<(PathBuf, (u32, u32), (u64, SystemTime))>,
) {
    let send = |path: PathBuf, w: u32, h: u32, file_size: u64, modified: SystemTime| {
        let _ = tx.send((path, (w, h), (file_size, modified)));
    };
    match mode {
        AppMode::Default => {
            let list =
                list_existing_image_paths_with_metadata(&watch_path, max_cards, extended_formats)
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
                let recent = crate::win_recent::list_recent_image_paths(
                    inspiration_recent_max,
                    extended_formats,
                );
                let screenshots = image_ops::list_screenshot_paths_ordered(&screenshot_dir);
                let mut paths: Vec<PathBuf> = recent;
                for p in screenshots {
                    if !paths.contains(&p) && paths.len() < inspiration_recent_max {
                        paths.push(p);
                    }
                }
                paths.truncate(inspiration_recent_max);
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
                    if !paths.contains(&p) && paths.len() < inspiration_recent_max {
                        paths.push(p);
                    }
                }
                paths.truncate(inspiration_recent_max);
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
            let list =
                list_existing_image_paths_with_metadata(&watch_path, max_cards, extended_formats)
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
pub(crate) fn run_texture_loader_thread(
    req_rx: mpsc::Receiver<PathBuf>,
    res_tx: mpsc::Sender<(PathBuf, Vec<u8>, u32, u32)>,
) {
    for path in req_rx {
        if let Ok((rgba, w, h)) = image_ops::load_image_rgba_for_texture(&path) {
            let _ = res_tx.send((path, rgba, w, h));
        }
    }
}
