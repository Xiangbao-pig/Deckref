//! DeckRef 入口：置顶透明窗口，初始化 eframe，处理错误，不使用 unwrap。

use deckref::{AppError, DeckRefApp};
use eframe::egui;
use std::path::PathBuf;

fn main() -> Result<(), AppError> {
    let watch_path = resolve_pictures_ref_dir()?;

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_transparent(true)
            .with_decorations(true)   // 标题栏：关闭、最小化按钮
            .with_resizable(true)     // 支持拖拽边框调整大小
            .with_always_on_top()
            .with_inner_size([360.0, 240.0]),
        ..Default::default()
    };

    let result = eframe::run_native(
        "DeckRef",
        options,
        Box::new(move |cc| {
            configure_cjk_fonts(cc);
            match create_app(watch_path) {
                Ok(app) => Ok(Box::new(app) as Box<dyn eframe::App>),
                Err(e) => Err(Box::new(e) as Box<dyn std::error::Error + Send + Sync>),
            }
        }),
    );

    result.map_err(|e| AppError::Other(e.to_string()))
}

/// 解析用户目录下的 Pictures/Ref 路径；若目录不存在则创建。
fn resolve_pictures_ref_dir() -> Result<PathBuf, AppError> {
    let user = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .map_err(AppError::EnvVar)?;
    let path = PathBuf::from(user).join("Pictures").join("Ref");
    std::fs::create_dir_all(&path).map_err(AppError::Io)?;
    Ok(path)
}

/// 配置中文字体，避免中文显示为方块。优先使用系统自带字体。
fn configure_cjk_fonts(cc: &eframe::CreationContext<'_>) {
    let mut fonts = egui::FontDefinitions::default();

    // Windows: 微软雅黑 / 宋体
    let font_paths = [
        "C:\\Windows\\Fonts\\msyh.ttc",
        "C:\\Windows\\Fonts\\msyhbd.ttc",
        "C:\\Windows\\Fonts\\simsun.ttc",
    ];
    for path in font_paths {
        if let Ok(data) = std::fs::read(path) {
            let font = egui::FontData::from_owned(data);
            fonts.font_data.insert("cjk".to_string(), font);
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .insert(0, "cjk".to_string());
            fonts
                .families
                .entry(egui::FontFamily::Monospace)
                .or_default()
                .insert(0, "cjk".to_string());
            break;
        }
    }

    cc.egui_ctx.set_fonts(fonts);
}

fn create_app(watch_path: PathBuf) -> Result<DeckRefApp, AppError> {
    DeckRefApp::new(watch_path)
}
