//! DeckRef 应用库：UI、文件监听、图片处理模块化分离。

pub mod app;
pub mod error;
pub mod image_card;
pub mod image_ops;
pub mod layout;
pub mod mode;
pub mod settings;
pub mod watcher;
pub mod window_state;

#[cfg(windows)]
pub mod win_drop;

#[cfg(windows)]
pub mod win_move;

#[cfg(windows)]
pub mod win_recent;

pub use app::DeckRefApp;
pub use error::AppError;
pub use image_card::{CardSortOrder, ImageCard, StackLayout};
pub use watcher::WatchEvent;
