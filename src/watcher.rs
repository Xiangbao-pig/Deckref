//! 文件系统监听模块。与 UI 完全解耦，通过 channel 发送事件。

use crate::error::{AppError, AppResult};
use crate::image_ops;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, SystemTime};

/// 判断路径是否为图片文件（按扩展名）。`extended` 为真时包含专业格式。
fn is_image_path(path: &Path, extended: bool) -> bool {
    image_ops::is_image_path_with(path, extended)
}

/// 监听产生的事件，供 UI 消费。
#[derive(Debug, Clone)]
pub enum WatchEvent {
    /// 文件或目录创建
    Create(PathBuf),
    /// 修改
    Modify(PathBuf),
    /// 删除
    Remove(PathBuf),
}

/// 配置：监听的根目录、防抖间隔、是否包含专业格式扩展名。
#[derive(Clone)]
pub struct WatcherConfig {
    pub watch_path: PathBuf,
    pub debounce_duration: Duration,
    /// 为 true 时监听专业/扩展格式（TGA、ICO、DDS、AVIF、EXR、HDR、PNM、QOI、FF 等）。
    pub extended_formats: bool,
}

impl Default for WatcherConfig {
    fn default() -> Self {
        Self {
            watch_path: PathBuf::from("."),
            debounce_duration: Duration::from_millis(400),
            extended_formats: false,
        }
    }
}

/// 在后台线程运行防抖监听，通过 `rx` 接收事件。  
/// 返回的 join handle 用于在程序退出时等待线程结束。
pub fn run_watcher(
    config: WatcherConfig,
    tx: mpsc::Sender<WatchEvent>,
) -> AppResult<std::thread::JoinHandle<AppResult<()>>> {
    let watch_path = config.watch_path.clone();
    let debounce = config.debounce_duration;
    let extended = config.extended_formats;

    let handle = std::thread::Builder::new()
        .name("deckref-watcher".into())
        .spawn(move || run_watcher_loop(watch_path, debounce, extended, tx))
        .map_err(|e| AppError::Other(e.to_string()))?;

    Ok(handle)
}

/// 扫描目录中已有图片路径（用于启动时加载），按修改时间新→旧排序。`extended` 为真时包含专业格式。
pub fn list_existing_image_paths(dir: &Path, extended: bool) -> AppResult<Vec<PathBuf>> {
    let with_meta = list_existing_image_paths_with_metadata(dir, usize::MAX, extended)?;
    Ok(with_meta.into_iter().map(|(p, _, _)| p).collect())
}

/// 扫描目录中已有图片路径并带元数据（一次 metadata 用于排序与构造卡片），按修改时间新→旧，最多 `max` 条。
/// `extended` 为真时包含专业格式扩展名。
pub fn list_existing_image_paths_with_metadata(
    dir: &Path,
    max: usize,
    extended: bool,
) -> AppResult<Vec<(PathBuf, u64, SystemTime)>> {
    let mut entries: Vec<(PathBuf, u64, SystemTime)> = std::fs::read_dir(dir)
        .map_err(AppError::Io)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file() && is_image_path(p, extended))
        .filter_map(|p| {
            std::fs::metadata(&p)
                .ok()
                .map(|m| (p, m.len(), m.modified().unwrap_or(SystemTime::UNIX_EPOCH)))
        })
        .collect();
    entries.sort_by(|a, b| b.2.cmp(&a.2));
    entries.truncate(max);
    Ok(entries)
}

fn run_watcher_loop(
    watch_path: PathBuf,
    debounce_duration: Duration,
    extended: bool,
    tx: mpsc::Sender<WatchEvent>,
) -> AppResult<()> {
    use notify::RecursiveMode;
    use notify_debouncer_mini::{new_debouncer, DebounceEventResult};

    let (debounce_tx, debounce_rx) = mpsc::channel();

    let mut debouncer = new_debouncer(debounce_duration, move |res: DebounceEventResult| {
        let _ = debounce_tx.send(res);
    })
    .map_err(AppError::Notify)?;

    debouncer
        .watcher()
        .watch(&watch_path, RecursiveMode::Recursive)
        .map_err(AppError::Notify)?;

    for res in debounce_rx {
        match res {
            Ok(events) => {
                for e in events {
                    if !is_image_path(&e.path, extended) {
                        continue;
                    }
                    let ev = WatchEvent::Modify(e.path);
                    if tx.send(ev).is_err() {
                        return Ok(());
                    }
                }
            }
            Err(e) => return Err(AppError::Notify(e)),
        }
    }

    Ok(())
}
