//! DeckRef 应用库：UI、文件监听、图片处理模块化分离。

pub mod app;
pub mod error;
pub mod image_card;
pub mod image_ops;
pub mod watcher;

#[cfg(windows)]
pub mod win_drop;

pub use app::DeckRefApp;
pub use error::AppError;
pub use image_card::{CardSortOrder, ImageCard, StackLayout};
pub use watcher::WatchEvent;
