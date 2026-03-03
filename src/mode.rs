//! 应用模式：不同模式对应不同监听目录与展示逻辑，便于后续扩展更多模式。

use std::path::{Path, PathBuf};

/// 应用模式：每种模式有独立的监听目录与展示含义。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AppMode {
    /// 默认模式：监听并展示「Ref」主文件夹中的图片。
    #[default]
    Default,

    /// 灵感模式：监听并展示「Ref/Inspiration」中的图片，适合最近收集的灵感图。
    Inspiration,
}

impl AppMode {
    /// 显示名称，用于 UI。
    pub fn label(self) -> &'static str {
        match self {
            AppMode::Default => "默认",
            AppMode::Inspiration => "灵感",
        }
    }

    /// 该模式对应的监听目录（基于应用根目录，如 Pictures/Ref）。
    /// 灵感模式使用子目录 Inspiration，便于与默认模式分离且易于备份。
    pub fn watch_path(self, base: &Path) -> PathBuf {
        match self {
            AppMode::Default => base.to_path_buf(),
            AppMode::Inspiration => base.join("Inspiration"),
        }
    }

    /// 确保该模式的监听目录存在；灵感模式需创建子目录。
    pub fn ensure_dir(self, base: &Path) -> std::io::Result<()> {
        let path = self.watch_path(base);
        std::fs::create_dir_all(&path)
    }

    /// 所有可用模式，用于遍历（如侧栏按钮）。
    pub fn all() -> &'static [AppMode] {
        &[AppMode::Default, AppMode::Inspiration]
    }
}
