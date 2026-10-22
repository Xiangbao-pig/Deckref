//! 后台加载、纹理解码与监听 IO 线程入口（与 UI 主线程通过 channel 通信）。

use crate::error::AppResult;
use crate::image_ops;
use crate::mode::AppMode;
use crate::watcher::list_existing_image_paths_with_metadata;
use crate::watcher::{WatchEvent, WatcherConfig};
#[cfg(not(windows))]
use crate::watcher::list_existing_image_paths;
use eframe::egui;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::SystemTime;

/// 后台线程在结果就绪时唤醒 UI。主线程每帧把当前 `Context` 写进来。
pub(crate) type RepaintHook = Arc<Mutex<Option<egui::Context>>>;

pub(crate) fn wake_ui(repaint: &RepaintHook) {
    if let Ok(guard) = repaint.lock() {
        if let Some(ctx) = guard.as_ref() {
            ctx.request_repaint();
        }
    }
}

/// 监听事件附带的尺寸与 metadata。主线程只用它构造卡片，不再自己读盘。
pub(crate) struct WatchIoDone {
    pub(crate) generation: u64,
    pub(crate) path: PathBuf,
    pub(crate) dimensions: Option<(u32, u32)>,
    pub(crate) file_size: u64,
    pub(crate) modified: SystemTime,
}

pub(crate) fn run_watch_io_thread(
    req_rx: mpsc::Receiver<(u64, PathBuf)>,
    res_tx: mpsc::Sender<WatchIoDone>,
    generation: Arc<AtomicU64>,
    repaint: RepaintHook,
) {
    while let Ok((requested_generation, path)) = req_rx.recv() {
        if requested_generation != generation.load(Ordering::Relaxed) {
            continue;
        }
        let dimensions = image_ops::image_dimensions(&path).ok();
        if requested_generation != generation.load(Ordering::Relaxed) {
            continue;
        }
        let (file_size, modified) = std::fs::metadata(&path)
            .map(|meta| (meta.len(), meta.modified().unwrap_or(SystemTime::UNIX_EPOCH)))
            .unwrap_or((0, SystemTime::UNIX_EPOCH));
        let done = WatchIoDone {
            generation: requested_generation,
            path,
            dimensions,
            file_size,
            modified,
        };
        if res_tx.send(done).is_err() {
            break;
        }
        wake_ui(&repaint);
    }
}

/// 启动目录监听，并在事件到达时唤醒 UI，避免静止画面收不到新文件。
pub(crate) fn spawn_waking_watcher(
    watch_path: PathBuf,
    extended_formats: bool,
    repaint: RepaintHook,
) -> AppResult<(mpsc::Receiver<WatchEvent>, thread::JoinHandle<AppResult<()>>)> {
    let (tx, rx) = mpsc::channel();
    let config = WatcherConfig {
        watch_path,
        extended_formats,
        ..WatcherConfig::default()
    };
    let handle = crate::watcher::run_watcher(config, tx)?;
    let (forward_tx, forward_rx) = mpsc::channel();
    thread::Builder::new()
        .name("deckref-watch-forward".into())
        .spawn(move || {
            while let Ok(event) = rx.recv() {
                if forward_tx.send(event).is_err() {
                    break;
                }
                wake_ui(&repaint);
            }
        })
        .map_err(|err| crate::error::AppError::Other(err.to_string()))?;
    Ok((forward_rx, handle))
}

pub(crate) fn run_loader_thread(
    mode: AppMode,
    watch_path: PathBuf,
    screenshot_dir: PathBuf,
    extended_formats: bool,
    max_cards: usize,
    inspiration_recent_max: usize,
    tx: mpsc::Sender<(PathBuf, (u32, u32), (u64, SystemTime))>,
    repaint: RepaintHook,
) {
    let send = |path: PathBuf, w: u32, h: u32, file_size: u64, modified: SystemTime| {
        if tx.send((path, (w, h), (file_size, modified))).is_ok() {
            wake_ui(&repaint);
        }
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

/// 后台纹理解码线程：从 req_rx 收 path 和目标最长边，解码为 RGBA 缩略图，通过 res_tx 回传；主线程只做 GPU 上传。
pub(crate) fn run_texture_loader_thread(
    req_rx: mpsc::Receiver<(PathBuf, u32)>,
    res_tx: mpsc::Sender<(PathBuf, Vec<u8>, u32, u32)>,
    repaint: RepaintHook,
) {
    for (path, max_edge) in req_rx {
        if let Ok((rgba, w, h)) = image_ops::load_image_rgba_for_texture(&path, max_edge) {
            if res_tx.send((path, rgba, w, h)).is_ok() {
                wake_ui(&repaint);
            }
        }
    }
}
