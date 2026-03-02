//! 文件系统监听模块。与 UI 完全解耦，通过 channel 发送事件。

use crate::error::{AppError, AppResult};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, SystemTime};

/// 常见图片扩展名（小写），用于过滤“新图片”事件与启动扫描。
pub const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "bmp", "webp", "tiff", "tif"];

/// 判断路径是否为图片文件（按扩展名）。
fn is_image_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| IMAGE_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
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

/// 配置：监听的根目录与防抖间隔。
#[derive(Clone)]
pub struct WatcherConfig {
    pub watch_path: PathBuf,
    pub debounce_duration: Duration,
}

impl Default for WatcherConfig {
    fn default() -> Self {
        Self {
            watch_path: PathBuf::from("."),
            debounce_duration: Duration::from_millis(400),
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

    let handle = std::thread::Builder::new()
        .name("deckref-watcher".into())
        .spawn(move || run_watcher_loop(watch_path, debounce, tx))
        .map_err(|e| AppError::Other(e.to_string()))?;

    Ok(handle)
}

/// 扫描目录中已有图片路径（用于启动时加载），按修改时间新→旧排序。
pub fn list_existing_image_paths(dir: &Path) -> AppResult<Vec<PathBuf>> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(AppError::Io)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file() && is_image_path(p))
        .collect();
    paths.sort_by(|a, b| {
        let ta = std::fs::metadata(a)
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let tb = std::fs::metadata(b)
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        tb.cmp(&ta)
    });
    Ok(paths)
}

fn run_watcher_loop(
    watch_path: PathBuf,
    debounce_duration: Duration,
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
                    if !is_image_path(&e.path) {
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
